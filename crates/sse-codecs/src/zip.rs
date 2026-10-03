//! Strict ZIP reader and reproducible writer (stored/deflate, ZIP64 read support).
use crate::{
    crc32::crc32,
    deflate::{compress_raw, Level},
    inflate::inflate_raw,
};
use std::io::Write;

use sse_core::{Error, Result};
/// One regular file stored in a ZIP archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Portable relative path using ZIP path syntax.
    pub name: String,
    /// Uncompressed file bytes.
    pub data: Vec<u8>,
}
fn u16le(b: &[u8], p: usize) -> Result<u16> {
    let s = b
        .get(p..p.saturating_add(2))
        .ok_or_else(|| Error::damaged("short ZIP u16"))?;
    Ok(u16::from_le_bytes(
        <[u8; 2]>::try_from(s).map_err(|_| Error::damaged("ZIP u16"))?,
    ))
}
fn u32le(b: &[u8], p: usize) -> Result<u32> {
    let s = b
        .get(p..p.saturating_add(4))
        .ok_or_else(|| Error::damaged("short ZIP u32"))?;
    Ok(u32::from_le_bytes(
        <[u8; 4]>::try_from(s).map_err(|_| Error::damaged("ZIP u32"))?,
    ))
}
fn u64le(b: &[u8], p: usize) -> Result<u64> {
    let s = b
        .get(p..p.saturating_add(8))
        .ok_or_else(|| Error::damaged("short ZIP u64"))?;
    Ok(u64::from_le_bytes(
        <[u8; 8]>::try_from(s).map_err(|_| Error::damaged("ZIP u64"))?,
    ))
}
fn safe_name(name: &str) -> bool {
    if name.is_empty() || name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    let bytes = name.as_bytes();
    if bytes.get(1) == Some(&b':') && bytes.first().is_some_and(u8::is_ascii_alphabetic) {
        return false;
    }
    !name.split(['/', '\\']).any(|x| x == "..")
}
fn cp437(bytes: &[u8]) -> String {
    const HI:&str="ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐└┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈıÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´≡±‗¾¶§÷¸°¨·¹³²■ ";
    let chars: Vec<char> = HI.chars().collect();
    let mut s = String::new();
    for x in bytes {
        if *x < 128 {
            s.push(char::from(*x));
        } else {
            s.push(chars.get(usize::from(*x).saturating_sub(128)).copied().unwrap_or('�'));
        }
    }
    s
}
fn name(bytes: &[u8], utf8: bool) -> Result<String> {
    let s = if utf8 {
        String::from_utf8(bytes.to_vec()).map_err(|_| Error::damaged("invalid ZIP UTF-8 name"))?
    } else {
        cp437(bytes)
    };
    if !safe_name(&s) {
        return Err(Error::Refused("unsafe ZIP path".to_owned()));
    }
    Ok(s)
}
fn zip64(extra: &[u8], need_u: bool, need_c: bool, need_o: bool) -> Result<(Option<u64>, Option<u64>, Option<u64>)> {
    let mut p = 0usize;
    while p.saturating_add(4) <= extra.len() {
        let id = u16le(extra, p)?;
        let n = usize::from(u16le(extra, p.saturating_add(2))?);
        let body = extra
            .get(p.saturating_add(4)..p.saturating_add(4).saturating_add(n))
            .ok_or_else(|| Error::damaged("short ZIP extra"))?;
        if id == 1 {
            let mut q = 0usize;
            let mut take = |need: bool| -> Result<Option<u64>> {
                if !need {
                    return Ok(None);
                }
                let v = u64le(body, q)?;
                q = q.saturating_add(8);
                Ok(Some(v))
            };
            return Ok((take(need_u)?, take(need_c)?, take(need_o)?));
        }
        p = p.saturating_add(4_usize.saturating_add(n));
    }
    Ok((None, None, None))
}
/// Reads all regular-file entries with a total decompressed byte ceiling.
pub fn read(input: &[u8], maximum_output: usize) -> Result<Vec<Entry>> {
    let start = input.len().saturating_sub(65_557);
    let eocd = (start..input.len().saturating_sub(3))
        .rev()
        .find(|p| input.get(*p..p.saturating_add(4)) == Some(&[0x50, 0x4b, 0x05, 0x06]))
        .ok_or_else(|| Error::damaged("ZIP EOCD not found"))?;
    let count = usize::from(u16le(input, eocd.saturating_add(10))?);
    let mut cd =
        usize::try_from(u32le(input, eocd.saturating_add(16))?).map_err(|_| Error::damaged("ZIP central offset"))?;
    let mut out = Vec::new();
    let mut total = 0usize;
    for _ in 0..count {
        if input.get(cd..cd.saturating_add(4)) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
            return Err(Error::damaged("ZIP central signature"));
        }
        let flags = u16le(input, cd.saturating_add(8))?;
        let method = u16le(input, cd.saturating_add(10))?;
        if flags & 1 != 0 {
            return Err(Error::Refused("encrypted ZIP entries are unsupported".to_owned()));
        }
        let crc = u32le(input, cd.saturating_add(16))?;
        let cs32 = u32le(input, cd.saturating_add(20))?;
        let us32 = u32le(input, cd.saturating_add(24))?;
        let nl = usize::from(u16le(input, cd.saturating_add(28))?);
        let xl = usize::from(u16le(input, cd.saturating_add(30))?);
        let cl = usize::from(u16le(input, cd.saturating_add(32))?);
        let attrs = u32le(input, cd.saturating_add(38))?;
        let off32 = u32le(input, cd.saturating_add(42))?;
        let nb = input
            .get(cd.saturating_add(46)..cd.saturating_add(46).saturating_add(nl))
            .ok_or_else(|| Error::damaged("ZIP name"))?;
        let extra = input
            .get(cd.saturating_add(46).saturating_add(nl)..cd.saturating_add(46).saturating_add(nl).saturating_add(xl))
            .ok_or_else(|| Error::damaged("ZIP extra"))?;
        let (zu, zc, zo) = zip64(extra, us32 == u32::MAX, cs32 == u32::MAX, off32 == u32::MAX)?;
        let us = usize::try_from(zu.unwrap_or(u64::from(us32))).map_err(|_| Error::damaged("ZIP size"))?;
        let cs = usize::try_from(zc.unwrap_or(u64::from(cs32))).map_err(|_| Error::damaged("ZIP size"))?;
        let off = usize::try_from(zo.unwrap_or(u64::from(off32))).map_err(|_| Error::damaged("ZIP offset"))?;
        let unix_mode = attrs.wrapping_shr(16) & 0xffff;
        if unix_mode & 0xf000 == 0xa000 {
            return Err(Error::Refused("ZIP symlink refused".to_owned()));
        }
        let n = name(nb, flags & 0x800 != 0)?;
        if n.ends_with('/') {
            cd = cd.saturating_add(46_usize.saturating_add(nl).saturating_add(xl).saturating_add(cl));
            continue;
        }
        total = total
            .checked_add(us)
            .ok_or_else(|| Error::damaged("ZIP output size overflow"))?;
        if total > maximum_output {
            return Err(Error::Refused("ZIP output limit exceeded".to_owned()));
        }
        if input.get(off..off.saturating_add(4)) != Some(&[0x50, 0x4b, 0x03, 0x04]) {
            return Err(Error::damaged("ZIP local signature"));
        }
        let lnl = usize::from(u16le(input, off.saturating_add(26))?);
        let lxl = usize::from(u16le(input, off.saturating_add(28))?);
        let ds = off
            .checked_add(30_usize.saturating_add(lnl).saturating_add(lxl))
            .ok_or_else(|| Error::damaged("ZIP data offset"))?;
        let comp = input
            .get(ds..ds.saturating_add(cs))
            .ok_or_else(|| Error::damaged("truncated ZIP data"))?;
        let data = match method {
            0 => comp.to_vec(),
            8 => inflate_raw(comp, us)?,
            _ => return Err(Error::Refused("unsupported ZIP compression method".to_owned())),
        };
        if data.len() != us || crc32(&data) != crc {
            return Err(Error::damaged("ZIP size/CRC mismatch"));
        }
        out.push(Entry { name: n, data });
        cd = cd
            .checked_add(46_usize.saturating_add(nl).saturating_add(xl).saturating_add(cl))
            .ok_or_else(|| Error::damaged("ZIP central overflow"))?;
    }
    Ok(out)
}
#[derive(Clone)]
struct Central {
    name: Vec<u8>,
    method: u16,
    crc: u32,
    compressed: u32,
    uncompressed: u32,
    offset: u32,
}

/// Streaming reproducible ZIP writer.
///
/// Local headers and payloads are written to the sink as entries arrive; only
/// compact central-directory metadata is retained until finish. Callers that
/// need name-order-independent reproducibility should use [write], which sorts
/// entries before feeding this writer.
pub struct Writer<W: Write> {
    sink: W,
    level: Level,
    offset: u64,
    central: Vec<Central>,
    finished: bool,
}

impl<W: Write> Writer<W> {
    /// Creates a writer using fixed DOS-epoch timestamps.
    pub fn new(sink: W, level: Level) -> Self {
        Self {
            sink,
            level,
            offset: 0,
            central: Vec::new(),
            finished: false,
        }
    }

    fn bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.sink.write_all(bytes)?;
        self.offset = self
            .offset
            .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        Ok(())
    }

    fn u16(&mut self, value: u16) -> Result<()> {
        self.bytes(&value.to_le_bytes())
    }

    fn u32(&mut self, value: u32) -> Result<()> {
        self.bytes(&value.to_le_bytes())
    }

    /// Adds one regular file and immediately writes its local record.
    pub fn add(&mut self, entry: &Entry) -> Result<()> {
        if self.finished {
            return Err(Error::Refused("ZIP writer already finished".to_owned()));
        }
        if !safe_name(&entry.name) || entry.name.ends_with('/') {
            return Err(Error::Refused("unsafe ZIP path".to_owned()));
        }
        let name = entry.name.as_bytes();
        let name_len = u16::try_from(name.len()).map_err(|_| Error::Refused("ZIP name too long".to_owned()))?;
        let compressed = compress_raw(&entry.data, self.level)?;
        let (method, payload) = if compressed.len() < entry.data.len() {
            (8_u16, compressed.as_slice())
        } else {
            (0_u16, entry.data.as_slice())
        };
        let offset = u32::try_from(self.offset)
            .map_err(|_| Error::Refused("ZIP64 writer is not needed by this package writer".to_owned()))?;
        let compressed_len =
            u32::try_from(payload.len()).map_err(|_| Error::Refused("ZIP entry too large".to_owned()))?;
        let uncompressed_len =
            u32::try_from(entry.data.len()).map_err(|_| Error::Refused("ZIP entry too large".to_owned()))?;
        let crc = crc32(&entry.data);

        self.u32(0x0403_4b50)?;
        self.u16(20)?;
        self.u16(0x0800)?;
        self.u16(method)?;
        self.u16(0)?;
        self.u16(0x0021)?;
        self.u32(crc)?;
        self.u32(compressed_len)?;
        self.u32(uncompressed_len)?;
        self.u16(name_len)?;
        self.u16(0)?;
        self.bytes(name)?;
        self.bytes(payload)?;
        self.central.push(Central {
            name: name.to_vec(),
            method,
            crc,
            compressed: compressed_len,
            uncompressed: uncompressed_len,
            offset,
        });
        Ok(())
    }

    /// Writes the central directory and returns the underlying sink.
    pub fn finish(mut self) -> Result<W> {
        if self.finished {
            return Err(Error::Refused("ZIP writer already finished".to_owned()));
        }
        self.finished = true;
        let directory_offset =
            u32::try_from(self.offset).map_err(|_| Error::Refused("ZIP archive too large".to_owned()))?;
        let central = std::mem::take(&mut self.central);
        for item in &central {
            let name_len =
                u16::try_from(item.name.len()).map_err(|_| Error::Refused("ZIP name too long".to_owned()))?;
            self.u32(0x0201_4b50)?;
            self.u16(0x0314)?;
            self.u16(20)?;
            self.u16(0x0800)?;
            self.u16(item.method)?;
            self.u16(0)?;
            self.u16(0x0021)?;
            self.u32(item.crc)?;
            self.u32(item.compressed)?;
            self.u32(item.uncompressed)?;
            self.u16(name_len)?;
            self.u16(0)?;
            self.u16(0)?;
            self.u16(0)?;
            self.u16(0)?;
            self.u32(0o100644_u32.wrapping_shl(16))?;
            self.u32(item.offset)?;
            self.bytes(&item.name)?;
        }
        let directory_size = u32::try_from(self.offset.saturating_sub(u64::from(directory_offset)))
            .map_err(|_| Error::Refused("ZIP central directory too large".to_owned()))?;
        let count = u16::try_from(central.len()).map_err(|_| Error::Refused("too many ZIP entries".to_owned()))?;
        self.u32(0x0605_4b50)?;
        self.u16(0)?;
        self.u16(0)?;
        self.u16(count)?;
        self.u16(count)?;
        self.u32(directory_size)?;
        self.u32(directory_offset)?;
        self.u16(0)?;
        Ok(self.sink)
    }
}

/// Reproducible ZIP writer: entries are sorted by UTF-8 name and use the DOS epoch timestamp.
pub fn write(entries: &[Entry], level: Level) -> Result<Vec<u8>> {
    let mut refs: Vec<&Entry> = entries.iter().collect();
    refs.sort_by(|left, right| left.name.cmp(&right.name));
    let mut writer = Writer::new(Vec::new(), level);
    for entry in refs {
        writer.add(entry)?;
    }
    writer.finish()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_stable() {
        let e = vec![
            Entry {
                name: "b.txt".to_owned(),
                data: vec![b'x'; 1000],
            },
            Entry {
                name: "a.txt".to_owned(),
                data: b"hello".to_vec(),
            },
        ];
        let a = write(&e, Level::Default).unwrap_or_default();
        let b = write(&e, Level::Default).unwrap_or_default();
        assert_eq!(a, b);
        let r = read(&a, 5000).unwrap_or_default();
        assert_eq!(r.len(), 2);
        assert_eq!(r.first().map(|x| x.name.as_str()), Some("a.txt"));
        assert_eq!(r.get(1).map(|x| x.data.len()), Some(1000));
    }

    #[test]
    fn streaming_writer_round_trips() {
        let mut writer = Writer::new(Vec::new(), Level::Fast);
        assert!(writer
            .add(&Entry {
                name: "a.bin".to_owned(),
                data: vec![7; 512]
            })
            .is_ok());
        let archive = writer.finish().unwrap_or_default();
        let files = read(&archive, 512).unwrap_or_default();
        assert_eq!(files.first().map(|entry| entry.data.len()), Some(512));
    }

    #[test]
    fn output_limit_stops_zip_bomb_shape() {
        let archive = write(
            &[Entry {
                name: "large".to_owned(),
                data: vec![0; 65_536],
            }],
            Level::Default,
        )
        .unwrap_or_default();
        assert!(matches!(read(&archive, 1024), Err(Error::Refused(_))));
    }
    #[test]
    fn traversal_refused() {
        let e = [Entry {
            name: "../x".to_owned(),
            data: Vec::new(),
        }];
        assert!(write(&e, Level::Fast).is_err());
    }
}
