use sse_core::{Error, Result};

const SIGNATURE: u32 = 0x504D_444D;
const HEADER_SIZE: usize = 32;
const DIRECTORY_ENTRY_SIZE: usize = 12;
const MAXIMUM_STREAMS: u32 = 1_024;
const MAXIMUM_MODULES: u32 = 4_096;
const MAXIMUM_THREADS: u32 = 65_536;
const MAXIMUM_MEMORY_RANGES: u64 = 1_048_576;
const MODULE_RECORD_SIZE: usize = 108;
const THREAD_RECORD_SIZE: usize = 48;
const SYSTEM_INFO_STREAM: u32 = 7;
const THREAD_LIST_STREAM: u32 = 3;
const MODULE_LIST_STREAM: u32 = 4;
const MEMORY_LIST_STREAM: u32 = 5;
const EXCEPTION_STREAM: u32 = 6;
const MEMORY64_LIST_STREAM: u32 = 9;

/// Processor architecture values used by Windows minidumps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    /// 32-bit x86.
    X86,
    /// 64-bit x86-64.
    X64,
    /// A value this reader does not interpret.
    Other(u16),
}

/// Selected fields from MINIDUMP_SYSTEM_INFO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemInfo {
    pub architecture: Architecture,
    pub processor_level: u16,
    pub processor_revision: u16,
    pub processor_count: u8,
    pub major_version: u32,
    pub minor_version: u32,
    pub build_number: u32,
    pub platform_id: u32,
}

/// The exception which caused the dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exception {
    pub thread_id: u32,
    pub code: u32,
    pub flags: u32,
    pub address: u64,
    context_size: u32,
    context_rva: u32,
}

/// One decoded thread context. Only registers needed for stack walking are exposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadContext {
    pub instruction_pointer: u64,
    pub stack_pointer: u64,
}

/// A module mapped into the crashed process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub base: u64,
    pub size: u32,
    pub timestamp: u32,
    pub version: (u16, u16, u16, u16),
    pub name: String,
}

/// A thread record. Stack bytes and context remain in the original dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thread {
    pub id: u32,
    pub stack_start: u64,
    stack_size: u32,
    stack_rva: u32,
    context_size: u32,
    context_rva: u32,
}

/// A stack address that belongs to a known module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub address: u64,
    pub module: String,
    pub offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StreamLocation {
    offset: usize,
    length: usize,
}

/// A checked, zero-copy view over a Windows minidump.
#[derive(Debug, Clone)]
pub struct Minidump<'a> {
    data: &'a [u8],
    stream_count: u32,
    directory_offset: usize,
}

impl<'a> Minidump<'a> {
    /// Parses the header and validates the complete stream directory.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        if data.len() < HEADER_SIZE {
            return Err(Error::damaged("minidump header is truncated"));
        }
        if read_u32(data, 0)? != SIGNATURE {
            return Err(Error::damaged("not a minidump file"));
        }

        let stream_count = read_u32(data, 8)?;
        if stream_count > MAXIMUM_STREAMS {
            return Err(Error::damaged("minidump stream count exceeds limit"));
        }
        let directory_offset = usize_from_u32(read_u32(data, 12)?)?;
        let directory_bytes = usize_from_u32(stream_count)?
            .checked_mul(DIRECTORY_ENTRY_SIZE)
            .ok_or_else(|| Error::damaged("minidump directory size overflow"))?;
        checked_range(data, directory_offset, directory_bytes, "minidump directory")?;

        let dump = Self {
            data,
            stream_count,
            directory_offset,
        };
        dump.validate_directories()?;
        Ok(dump)
    }

    /// Returns the SystemInfo stream when present.
    pub fn system_info(&self) -> Result<Option<SystemInfo>> {
        let Some(stream) = self.find_stream(SYSTEM_INFO_STREAM)? else {
            return Ok(None);
        };
        if stream.length < 32 {
            return Err(Error::damaged("SystemInfo stream is truncated"));
        }
        let bytes = checked_range(self.data, stream.offset, stream.length, "SystemInfo stream")?;
        let raw_arch = read_u16(bytes, 0)?;
        let architecture = match raw_arch {
            0 => Architecture::X86,
            9 => Architecture::X64,
            other => Architecture::Other(other),
        };
        Ok(Some(SystemInfo {
            architecture,
            processor_level: read_u16(bytes, 2)?,
            processor_revision: read_u16(bytes, 4)?,
            processor_count: read_u8(bytes, 6)?,
            major_version: read_u32(bytes, 8)?,
            minor_version: read_u32(bytes, 12)?,
            build_number: read_u32(bytes, 16)?,
            platform_id: read_u32(bytes, 20)?,
        }))
    }

    /// Returns the crash exception when present.
    pub fn exception(&self) -> Result<Option<Exception>> {
        let Some(stream) = self.find_stream(EXCEPTION_STREAM)? else {
            return Ok(None);
        };
        if stream.length < 168 {
            return Err(Error::damaged("Exception stream is truncated"));
        }
        let bytes = checked_range(self.data, stream.offset, stream.length, "Exception stream")?;
        let exception = Exception {
            thread_id: read_u32(bytes, 0)?,
            code: read_u32(bytes, 8)?,
            flags: read_u32(bytes, 12)?,
            address: read_u64(bytes, 24)?,
            context_size: read_u32(bytes, 160)?,
            context_rva: read_u32(bytes, 164)?,
        };
        self.validate_location(exception.context_rva, exception.context_size, "exception context")?;
        Ok(Some(exception))
    }

    /// Iterates modules without copying stream bytes.
    pub fn modules(&self) -> Result<ModuleIter<'a, '_>> {
        let stream = self.find_stream(MODULE_LIST_STREAM)?;
        let count = if let Some(location) = stream {
            let bytes = checked_range(self.data, location.offset, location.length, "ModuleList stream")?;
            if bytes.len() < 4 {
                return Err(Error::damaged("ModuleList stream is truncated"));
            }
            let count = read_u32(bytes, 0)?;
            if count > MAXIMUM_MODULES {
                return Err(Error::damaged("module count exceeds limit"));
            }
            let records = usize_from_u32(count)?
                .checked_mul(MODULE_RECORD_SIZE)
                .and_then(|value| value.checked_add(4))
                .ok_or_else(|| Error::damaged("module list size overflow"))?;
            if records > bytes.len() {
                return Err(Error::damaged("ModuleList records are truncated"));
            }
            count
        } else {
            0
        };
        Ok(ModuleIter {
            dump: self,
            stream,
            count,
            index: 0,
        })
    }

    /// Iterates threads without copying stream bytes.
    pub fn threads(&self) -> Result<ThreadIter<'a, '_>> {
        let stream = self.find_stream(THREAD_LIST_STREAM)?;
        let count = if let Some(location) = stream {
            let bytes = checked_range(self.data, location.offset, location.length, "ThreadList stream")?;
            if bytes.len() < 4 {
                return Err(Error::damaged("ThreadList stream is truncated"));
            }
            let count = read_u32(bytes, 0)?;
            if count > MAXIMUM_THREADS {
                return Err(Error::damaged("thread count exceeds limit"));
            }
            let records = usize_from_u32(count)?
                .checked_mul(THREAD_RECORD_SIZE)
                .and_then(|value| value.checked_add(4))
                .ok_or_else(|| Error::damaged("thread list size overflow"))?;
            if records > bytes.len() {
                return Err(Error::damaged("ThreadList records are truncated"));
            }
            count
        } else {
            0
        };
        Ok(ThreadIter {
            dump: self,
            stream,
            count,
            index: 0,
        })
    }

    /// Context from the exception stream, interpreted using SystemInfo.
    pub fn faulting_context(&self) -> Result<Option<ThreadContext>> {
        let Some(exception) = self.exception()? else {
            return Ok(None);
        };
        let Some(system) = self.system_info()? else {
            return Ok(None);
        };
        self.decode_context(exception.context_rva, exception.context_size, system.architecture)
            .map(Some)
    }

    /// Context from a ThreadList record.
    pub fn thread_context(&self, thread: Thread) -> Result<Option<ThreadContext>> {
        let Some(system) = self.system_info()? else {
            return Ok(None);
        };
        self.decode_context(thread.context_rva, thread.context_size, system.architecture)
            .map(Some)
    }

    /// Returns a memory view containing `address`, checking MemoryList and Memory64List.
    pub fn memory_at(&self, address: u64, maximum: usize) -> Result<Option<&'a [u8]>> {
        if let Some(bytes) = self.memory_list_at(address, maximum)? {
            return Ok(Some(bytes));
        }
        self.memory64_list_at(address, maximum)
    }

    /// Walks the faulting stack conservatively by treating pointer-sized stack words as candidate return addresses.
    /// The exception address is emitted first when it belongs to a known module. Duplicate adjacent frames are omitted.
    pub fn faulting_stack(&self, maximum_frames: usize) -> Result<Vec<Frame>> {
        let mut frames = Vec::new();
        if maximum_frames == 0 {
            return Ok(frames);
        }
        let modules = collect_modules(self)?;
        let Some(exception) = self.exception()? else {
            return Ok(frames);
        };
        if let Some(frame) = frame_for_address(&modules, exception.address) {
            frames.push(frame);
        }
        if frames.len() >= maximum_frames {
            return Ok(frames);
        }
        let Some(system) = self.system_info()? else {
            return Ok(frames);
        };
        let Some(context) = self.faulting_context()? else {
            return Ok(frames);
        };
        let pointer_size = match system.architecture {
            Architecture::X86 => 4,
            Architecture::X64 => 8,
            Architecture::Other(_) => return Ok(frames),
        };
        let Some(stack) = self.memory_at(context.stack_pointer, 8 * 1_024 * 1_024)? else {
            return Ok(frames);
        };
        let mut position = 0_usize;
        while position.checked_add(pointer_size).is_some_and(|end| end <= stack.len()) && frames.len() < maximum_frames
        {
            let candidate = if pointer_size == 4 {
                u64::from(read_u32(stack, position)?)
            } else {
                read_u64(stack, position)?
            };
            if let Some(frame) = frame_for_address(&modules, candidate) {
                let duplicate = frames.last().is_some_and(|previous| previous.address == frame.address);
                if !duplicate {
                    frames.push(frame);
                }
            }
            position = position
                .checked_add(pointer_size)
                .ok_or_else(|| Error::damaged("stack scan offset overflow"))?;
        }
        Ok(frames)
    }

    fn validate_directories(&self) -> Result<()> {
        let mut outer = 0_u32;
        while outer < self.stream_count {
            let outer_location = self.directory_location(outer)?;
            let mut inner = outer
                .checked_add(1)
                .ok_or_else(|| Error::damaged("directory index overflow"))?;
            while inner < self.stream_count {
                let inner_location = self.directory_location(inner)?;
                if ranges_overlap(outer_location, inner_location) {
                    return Err(Error::damaged("minidump streams overlap"));
                }
                inner = inner
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("directory index overflow"))?;
            }
            outer = outer
                .checked_add(1)
                .ok_or_else(|| Error::damaged("directory index overflow"))?;
        }
        Ok(())
    }

    fn directory_location(&self, index: u32) -> Result<StreamLocation> {
        let entry_offset = usize_from_u32(index)?
            .checked_mul(DIRECTORY_ENTRY_SIZE)
            .and_then(|value| value.checked_add(self.directory_offset))
            .ok_or_else(|| Error::damaged("directory entry offset overflow"))?;
        let entry = checked_range(self.data, entry_offset, DIRECTORY_ENTRY_SIZE, "directory entry")?;
        let length = usize_from_u32(read_u32(entry, 4)?)?;
        let offset = usize_from_u32(read_u32(entry, 8)?)?;
        checked_range(self.data, offset, length, "minidump stream")?;
        Ok(StreamLocation { offset, length })
    }

    fn find_stream(&self, stream_type: u32) -> Result<Option<StreamLocation>> {
        let mut index = 0_u32;
        let mut found = None;
        while index < self.stream_count {
            let entry_offset = usize_from_u32(index)?
                .checked_mul(DIRECTORY_ENTRY_SIZE)
                .and_then(|value| value.checked_add(self.directory_offset))
                .ok_or_else(|| Error::damaged("directory entry offset overflow"))?;
            let entry = checked_range(self.data, entry_offset, DIRECTORY_ENTRY_SIZE, "directory entry")?;
            if read_u32(entry, 0)? == stream_type {
                if found.is_some() {
                    return Err(Error::damaged("duplicate minidump stream type"));
                }
                found = Some(self.directory_location(index)?);
            }
            index = index
                .checked_add(1)
                .ok_or_else(|| Error::damaged("directory index overflow"))?;
        }
        Ok(found)
    }

    fn validate_location(&self, rva: u32, size: u32, what: &str) -> Result<()> {
        let offset = usize_from_u32(rva)?;
        let length = usize_from_u32(size)?;
        checked_range(self.data, offset, length, what).map(|_| ())
    }

    fn decode_context(&self, rva: u32, size: u32, architecture: Architecture) -> Result<ThreadContext> {
        self.validate_location(rva, size, "thread context")?;
        let bytes = checked_range(self.data, usize_from_u32(rva)?, usize_from_u32(size)?, "thread context")?;
        match architecture {
            Architecture::X64 => {
                if bytes.len() < 256 {
                    return Err(Error::damaged("x64 CONTEXT is truncated"));
                }
                Ok(ThreadContext {
                    stack_pointer: read_u64(bytes, 152)?,
                    instruction_pointer: read_u64(bytes, 248)?,
                })
            }
            Architecture::X86 => {
                if bytes.len() < 204 {
                    return Err(Error::damaged("x86 CONTEXT is truncated"));
                }
                Ok(ThreadContext {
                    instruction_pointer: u64::from(read_u32(bytes, 184)?),
                    stack_pointer: u64::from(read_u32(bytes, 196)?),
                })
            }
            Architecture::Other(_) => Err(Error::damaged("unsupported minidump CPU architecture")),
        }
    }

    fn memory_list_at(&self, address: u64, maximum: usize) -> Result<Option<&'a [u8]>> {
        let Some(stream) = self.find_stream(MEMORY_LIST_STREAM)? else {
            return Ok(None);
        };
        let bytes = checked_range(self.data, stream.offset, stream.length, "MemoryList stream")?;
        if bytes.len() < 4 {
            return Err(Error::damaged("MemoryList stream is truncated"));
        }
        let count = read_u32(bytes, 0)?;
        if u64::from(count) > MAXIMUM_MEMORY_RANGES {
            return Err(Error::damaged("MemoryList count exceeds limit"));
        }
        let required = usize_from_u32(count)?
            .checked_mul(16)
            .and_then(|value| value.checked_add(4))
            .ok_or_else(|| Error::damaged("MemoryList size overflow"))?;
        if required > bytes.len() {
            return Err(Error::damaged("MemoryList descriptors are truncated"));
        }
        let mut index = 0_u32;
        while index < count {
            let position = usize_from_u32(index)?
                .checked_mul(16)
                .and_then(|value| value.checked_add(4))
                .ok_or_else(|| Error::damaged("MemoryList descriptor offset overflow"))?;
            let start = read_u64(bytes, position)?;
            let size_position = position
                .checked_add(8)
                .ok_or_else(|| Error::damaged("MemoryList descriptor overflow"))?;
            let rva_position = position
                .checked_add(12)
                .ok_or_else(|| Error::damaged("MemoryList descriptor overflow"))?;
            let size = read_u32(bytes, size_position)?;
            let rva = read_u32(bytes, rva_position)?;
            if let Some(result) = memory_subslice(self.data, address, start, u64::from(size), u64::from(rva), maximum)?
            {
                return Ok(Some(result));
            }
            index = index
                .checked_add(1)
                .ok_or_else(|| Error::damaged("MemoryList index overflow"))?;
        }
        Ok(None)
    }

    fn memory64_list_at(&self, address: u64, maximum: usize) -> Result<Option<&'a [u8]>> {
        let Some(stream) = self.find_stream(MEMORY64_LIST_STREAM)? else {
            return Ok(None);
        };
        let bytes = checked_range(self.data, stream.offset, stream.length, "Memory64List stream")?;
        if bytes.len() < 16 {
            return Err(Error::damaged("Memory64List stream is truncated"));
        }
        let count = read_u64(bytes, 0)?;
        if count > MAXIMUM_MEMORY_RANGES {
            return Err(Error::damaged("Memory64List count exceeds limit"));
        }
        let count_usize =
            usize::try_from(count).map_err(|_| Error::damaged("Memory64List count does not fit usize"))?;
        let required = count_usize
            .checked_mul(16)
            .and_then(|value| value.checked_add(16))
            .ok_or_else(|| Error::damaged("Memory64List descriptor size overflow"))?;
        if required > bytes.len() {
            return Err(Error::damaged("Memory64List descriptors are truncated"));
        }
        let mut data_rva = read_u64(bytes, 8)?;
        let mut index = 0_usize;
        while index < count_usize {
            let position = index
                .checked_mul(16)
                .and_then(|value| value.checked_add(16))
                .ok_or_else(|| Error::damaged("Memory64List descriptor offset overflow"))?;
            let start = read_u64(bytes, position)?;
            let size_position = position
                .checked_add(8)
                .ok_or_else(|| Error::damaged("Memory64List descriptor overflow"))?;
            let size = read_u64(bytes, size_position)?;
            if let Some(result) = memory_subslice(self.data, address, start, size, data_rva, maximum)? {
                return Ok(Some(result));
            }
            data_rva = data_rva
                .checked_add(size)
                .ok_or_else(|| Error::damaged("Memory64List data offset overflow"))?;
            index = index
                .checked_add(1)
                .ok_or_else(|| Error::damaged("Memory64List index overflow"))?;
        }
        Ok(None)
    }
}

/// Iterator over checked ModuleList records.
pub struct ModuleIter<'a, 'd> {
    dump: &'d Minidump<'a>,
    stream: Option<StreamLocation>,
    count: u32,
    index: u32,
}

impl Iterator for ModuleIter<'_, '_> {
    type Item = Result<Module>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }
        let result = self.read_current();
        self.index = self.index.saturating_add(1);
        Some(result)
    }
}

impl ModuleIter<'_, '_> {
    fn read_current(&self) -> Result<Module> {
        let stream = self.stream.ok_or_else(|| Error::damaged("missing ModuleList stream"))?;
        let stream_bytes = checked_range(self.dump.data, stream.offset, stream.length, "ModuleList stream")?;
        let record_offset = usize_from_u32(self.index)?
            .checked_mul(MODULE_RECORD_SIZE)
            .and_then(|value| value.checked_add(4))
            .ok_or_else(|| Error::damaged("module record offset overflow"))?;
        let record = checked_range(stream_bytes, record_offset, MODULE_RECORD_SIZE, "module record")?;
        let name_rva = read_u32(record, 20)?;
        let name = read_utf16_string(self.dump.data, name_rva, 2_048)?;
        let file_version_ms = read_u32(record, 32)?;
        let file_version_ls = read_u32(record, 36)?;
        Ok(Module {
            base: read_u64(record, 0)?,
            size: read_u32(record, 8)?,
            timestamp: read_u32(record, 16)?,
            version: (
                u16::try_from(file_version_ms >> 16).map_err(|_| Error::damaged("module version overflow"))?,
                u16::try_from(file_version_ms & 0xFFFF).map_err(|_| Error::damaged("module version overflow"))?,
                u16::try_from(file_version_ls >> 16).map_err(|_| Error::damaged("module version overflow"))?,
                u16::try_from(file_version_ls & 0xFFFF).map_err(|_| Error::damaged("module version overflow"))?,
            ),
            name,
        })
    }
}

/// Iterator over checked ThreadList records.
pub struct ThreadIter<'a, 'd> {
    dump: &'d Minidump<'a>,
    stream: Option<StreamLocation>,
    count: u32,
    index: u32,
}

impl Iterator for ThreadIter<'_, '_> {
    type Item = Result<Thread>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }
        let result = self.read_current();
        self.index = self.index.saturating_add(1);
        Some(result)
    }
}

impl ThreadIter<'_, '_> {
    fn read_current(&self) -> Result<Thread> {
        let stream = self.stream.ok_or_else(|| Error::damaged("missing ThreadList stream"))?;
        let stream_bytes = checked_range(self.dump.data, stream.offset, stream.length, "ThreadList stream")?;
        let record_offset = usize_from_u32(self.index)?
            .checked_mul(THREAD_RECORD_SIZE)
            .and_then(|value| value.checked_add(4))
            .ok_or_else(|| Error::damaged("thread record offset overflow"))?;
        let record = checked_range(stream_bytes, record_offset, THREAD_RECORD_SIZE, "thread record")?;
        let thread = Thread {
            id: read_u32(record, 0)?,
            stack_start: read_u64(record, 24)?,
            stack_size: read_u32(record, 32)?,
            stack_rva: read_u32(record, 36)?,
            context_size: read_u32(record, 40)?,
            context_rva: read_u32(record, 44)?,
        };
        self.dump
            .validate_location(thread.stack_rva, thread.stack_size, "thread stack")?;
        self.dump
            .validate_location(thread.context_rva, thread.context_size, "thread context")?;
        Ok(thread)
    }
}

fn collect_modules(dump: &Minidump<'_>) -> Result<Vec<Module>> {
    let mut modules = Vec::new();
    for module in dump.modules()? {
        modules.push(module?);
    }
    Ok(modules)
}

fn frame_for_address(modules: &[Module], address: u64) -> Option<Frame> {
    for module in modules {
        let end = module.base.checked_add(u64::from(module.size))?;
        if address >= module.base && address < end {
            let offset = address.checked_sub(module.base)?;
            return Some(Frame {
                address,
                module: module.name.clone(),
                offset,
            });
        }
    }
    None
}

fn memory_subslice<'a>(
    data: &'a [u8],
    address: u64,
    start: u64,
    size: u64,
    file_rva: u64,
    maximum: usize,
) -> Result<Option<&'a [u8]>> {
    let end = start
        .checked_add(size)
        .ok_or_else(|| Error::damaged("memory range address overflow"))?;
    if address < start || address >= end {
        return Ok(None);
    }
    let delta = address
        .checked_sub(start)
        .ok_or_else(|| Error::damaged("memory range delta underflow"))?;
    let available = size
        .checked_sub(delta)
        .ok_or_else(|| Error::damaged("memory range size underflow"))?;
    let maximum_u64 = u64::try_from(maximum).map_err(|_| Error::damaged("maximum memory read does not fit u64"))?;
    let wanted = available.min(maximum_u64);
    let file_start = file_rva
        .checked_add(delta)
        .ok_or_else(|| Error::damaged("memory file offset overflow"))?;
    let file_start_usize =
        usize::try_from(file_start).map_err(|_| Error::damaged("memory file offset does not fit usize"))?;
    let wanted_usize = usize::try_from(wanted).map_err(|_| Error::damaged("memory read size does not fit usize"))?;
    Ok(Some(checked_range(
        data,
        file_start_usize,
        wanted_usize,
        "memory bytes",
    )?))
}

fn read_utf16_string(data: &[u8], rva: u32, maximum_bytes: usize) -> Result<String> {
    let offset = usize_from_u32(rva)?;
    let byte_length = usize_from_u32(read_u32(data, offset)?)?;
    if byte_length > maximum_bytes || byte_length % 2 != 0 {
        return Err(Error::damaged("invalid minidump UTF-16 string length"));
    }
    let text_offset = offset
        .checked_add(4)
        .ok_or_else(|| Error::damaged("UTF-16 string offset overflow"))?;
    let bytes = checked_range(data, text_offset, byte_length, "UTF-16 string")?;
    let mut words = Vec::with_capacity(byte_length / 2);
    let mut position = 0_usize;
    while position < byte_length {
        words.push(read_u16(bytes, position)?);
        position = position
            .checked_add(2)
            .ok_or_else(|| Error::damaged("UTF-16 string offset overflow"))?;
    }
    let path = String::from_utf16(&words).map_err(|_| Error::damaged("invalid UTF-16 module name"))?;
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path.as_str()).to_owned();
    Ok(name)
}

fn ranges_overlap(left: StreamLocation, right: StreamLocation) -> bool {
    if left.length == 0 || right.length == 0 {
        return false;
    }
    let Some(left_end) = left.offset.checked_add(left.length) else {
        return true;
    };
    let Some(right_end) = right.offset.checked_add(right.length) else {
        return true;
    };
    left.offset < right_end && right.offset < left_end
}

fn checked_range<'a>(data: &'a [u8], offset: usize, length: usize, what: &str) -> Result<&'a [u8]> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| Error::damaged(format!("{what} range overflow")))?;
    data.get(offset..end)
        .ok_or_else(|| Error::damaged(format!("{what} lies beyond the file")))
}

fn read_u8(data: &[u8], offset: usize) -> Result<u8> {
    data.get(offset)
        .copied()
        .ok_or_else(|| Error::damaged("byte read beyond minidump"))
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16> {
    let bytes = checked_range(data, offset, 2, "u16")?;
    let array = <[u8; 2]>::try_from(bytes).map_err(|_| Error::damaged("invalid u16 bytes"))?;
    Ok(u16::from_le_bytes(array))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = checked_range(data, offset, 4, "u32")?;
    let array = <[u8; 4]>::try_from(bytes).map_err(|_| Error::damaged("invalid u32 bytes"))?;
    Ok(u32::from_le_bytes(array))
}

fn read_u64(data: &[u8], offset: usize) -> Result<u64> {
    let bytes = checked_range(data, offset, 8, "u64")?;
    let array = <[u8; 8]>::try_from(bytes).map_err(|_| Error::damaged("invalid u64 bytes"))?;
    Ok(u64::from_le_bytes(array))
}

fn usize_from_u32(value: u32) -> Result<usize> {
    usize::try_from(value).map_err(|_| Error::damaged("u32 value does not fit usize"))
}

#[cfg(test)]
mod tests {
    use super::{Architecture, Minidump, MODULE_LIST_STREAM, SIGNATURE};

    fn put_u16(data: &mut [u8], offset: usize, value: u16) {
        if let Some(target) = data.get_mut(offset..offset.saturating_add(2)) {
            target.copy_from_slice(&value.to_le_bytes());
        }
    }

    fn put_u32(data: &mut [u8], offset: usize, value: u32) {
        if let Some(target) = data.get_mut(offset..offset.saturating_add(4)) {
            target.copy_from_slice(&value.to_le_bytes());
        }
    }

    fn put_u64(data: &mut [u8], offset: usize, value: u64) {
        if let Some(target) = data.get_mut(offset..offset.saturating_add(8)) {
            target.copy_from_slice(&value.to_le_bytes());
        }
    }

    fn write_entry(data: &mut [u8], offset: usize, stream_type: u32, size: u32, rva: u32) {
        put_u32(data, offset, stream_type);
        put_u32(data, offset.saturating_add(4), size);
        put_u32(data, offset.saturating_add(8), rva);
    }

    fn build_reference_dump(code: u32, address: u64) -> Vec<u8> {
        let name_utf16: Vec<u16> = r"S:\game\bin\xrCore.dll".encode_utf16().collect();
        let mut name = Vec::new();
        for word in name_utf16 {
            name.extend_from_slice(&word.to_le_bytes());
        }
        let header = 32_usize;
        let directory = 24_usize;
        let modules = 112_usize;
        let exception = 168_usize;
        let name_offset = header
            .checked_add(directory)
            .and_then(|value| value.checked_add(modules))
            .and_then(|value| value.checked_add(exception))
            .unwrap_or_default();
        let text_offset = name_offset
            .checked_add(4)
            .and_then(|value| value.checked_add(name.len()))
            .and_then(|value| value.checked_add(8))
            .unwrap_or_default();
        let text = b"Expression    : fatal error\0Function      : CInifile::r_section\0Line          : 443\0Description   : <no expression>\0Arguments     : Can't open section 'doc_5'\0";
        let total = text_offset
            .checked_add(text.len())
            .and_then(|value| value.checked_add(16))
            .unwrap_or_default();
        let mut dump = vec![0_u8; total];
        put_u32(&mut dump, 0, SIGNATURE);
        put_u32(&mut dump, 8, 2);
        put_u32(&mut dump, 12, 32);
        write_entry(&mut dump, header, 4, 112, 56);
        write_entry(&mut dump, header.saturating_add(12), 6, 168, 168);
        put_u32(&mut dump, 56, 1);
        put_u64(&mut dump, 60, 0x1_0000_0000);
        put_u32(&mut dump, 68, 0x20_0000);
        put_u32(&mut dump, 76, u32::try_from(name_offset).unwrap_or_default());
        put_u32(&mut dump, 176, code);
        put_u64(&mut dump, 192, address);
        put_u32(&mut dump, name_offset, u32::try_from(name.len()).unwrap_or_default());
        if let Some(target) =
            dump.get_mut(name_offset.saturating_add(4)..name_offset.saturating_add(4).saturating_add(name.len()))
        {
            target.copy_from_slice(&name);
        }
        if let Some(target) = dump.get_mut(text_offset..text_offset.saturating_add(text.len())) {
            target.copy_from_slice(text);
        }
        dump
    }

    #[test]
    fn csharp_reference_dump_reads_exception_and_module() {
        let dump = build_reference_dump(0x8000_0003, 0x1_0001_B944);
        let parsed = Minidump::parse(&dump);
        assert!(parsed.is_ok());
        let parsed = match parsed {
            Ok(value) => value,
            Err(error) => panic!("unexpected parse error: {error}"),
        };
        let exception = parsed.exception();
        assert!(exception.is_ok());
        let exception = match exception {
            Ok(Some(value)) => value,
            other => panic!("missing exception: {other:?}"),
        };
        assert_eq!(exception.code, 0x8000_0003);
        assert_eq!(exception.address, 0x1_0001_B944);
        let modules = parsed.modules();
        assert!(modules.is_ok());
        let mut modules = match modules {
            Ok(value) => value,
            Err(error) => panic!("module iterator error: {error}"),
        };
        let module = match modules.next() {
            Some(Ok(value)) => value,
            other => panic!("missing module: {other:?}"),
        };
        assert_eq!(module.name, "xrCore.dll");
        assert_eq!(module.base, 0x1_0000_0000);
        assert_eq!(module.size, 0x20_0000);
    }

    #[test]
    fn access_violation_address_maps_to_reference_module() {
        let dump = build_reference_dump(0xC000_0005, 0x1_0008_79A4);
        let parsed = match Minidump::parse(&dump) {
            Ok(value) => value,
            Err(error) => panic!("unexpected parse error: {error}"),
        };
        let modules: Vec<_> = match parsed.modules() {
            Ok(iter) => iter.filter_map(std::result::Result::ok).collect(),
            Err(error) => panic!("module iterator error: {error}"),
        };
        let module = modules.first();
        assert!(module.is_some());
        let module = match module {
            Some(value) => value,
            None => panic!("module missing"),
        };
        assert_eq!(0x1_0008_79A4_u64.checked_sub(module.base), Some(0x879A4));
    }

    #[test]
    fn truncation_at_every_sixty_fourth_byte_is_an_error_not_a_panic() {
        let dump = build_reference_dump(0xC000_0005, 0x1_0008_79A4);
        let mut length = 0_usize;
        while length < dump.len() {
            let prefix = dump.get(..length).unwrap_or_default();
            assert!(Minidump::parse(prefix).is_err());
            length = length.saturating_add(64);
        }
    }

    #[test]
    fn a_dump_claiming_u32_max_modules_is_rejected() {
        let mut dump = build_reference_dump(0xC000_0005, 0x1_0008_79A4);
        let directory = 32_usize;
        let module_entry = dump.get(directory..directory.saturating_add(12));
        assert!(module_entry.is_some());
        let stream_rva = match module_entry {
            Some(entry) => {
                u32::from_le_bytes(<[u8; 4]>::try_from(entry.get(8..12).unwrap_or_default()).unwrap_or_default())
            }
            None => 0,
        };
        put_u32(&mut dump, usize::try_from(stream_rva).unwrap_or_default(), u32::MAX);
        let parsed = match Minidump::parse(&dump) {
            Ok(value) => value,
            Err(error) => panic!("header should still parse: {error}"),
        };
        assert!(parsed.modules().is_err());
    }

    #[test]
    fn system_info_x64_context_offsets_are_decoded() {
        let mut dump = vec![
            0_u8;
            32_usize
                .saturating_add(24)
                .saturating_add(56)
                .saturating_add(168)
                .saturating_add(256)
        ];
        put_u32(&mut dump, 0, SIGNATURE);
        put_u32(&mut dump, 8, 2);
        put_u32(&mut dump, 12, 32);
        write_entry(&mut dump, 32, 7, 56, 56);
        write_entry(&mut dump, 44, 6, 168, 112);
        put_u16(&mut dump, 56, 9);
        if let Some(slot) = dump.get_mut(62) {
            *slot = 8;
        }
        put_u32(&mut dump, 120, 0xC000_0005);
        put_u32(&mut dump, 272, 256);
        put_u32(&mut dump, 276, 280);
        put_u64(&mut dump, 432, 0x1234_5000);
        put_u64(&mut dump, 528, 0x1234_5678);
        let parsed = match Minidump::parse(&dump) {
            Ok(value) => value,
            Err(error) => panic!("unexpected parse error: {error}"),
        };
        let info = match parsed.system_info() {
            Ok(Some(value)) => value,
            other => panic!("missing system info: {other:?}"),
        };
        assert_eq!(info.architecture, Architecture::X64);
        let context = match parsed.faulting_context() {
            Ok(Some(value)) => value,
            other => panic!("missing context: {other:?}"),
        };
        assert_eq!(context.stack_pointer, 0x1234_5000);
        assert_eq!(context.instruction_pointer, 0x1234_5678);
    }

    #[test]
    fn overlapping_streams_are_rejected() {
        let mut dump = build_reference_dump(0xC000_0005, 0x1_0008_79A4);
        write_entry(&mut dump, 44, MODULE_LIST_STREAM, 168, 56);
        assert!(Minidump::parse(&dump).is_err());
    }
}
