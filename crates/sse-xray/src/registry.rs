//! Shared reader for the actor registries in the ALife registry chunk (9): tasks and statistics.
//!
//! Port of the reader in `XRayProgressReader` from the C# 1.3.1 core. Strings are zero-terminated, at most
//! `MAXIMUM_STRING` bytes long and free of control characters; any other malformed field stops the parse.

/// Longest zero-terminated string accepted in a registry (the C# reader's limit).
pub(crate) const MAXIMUM_STRING: usize = 4096;
/// Largest element count accepted in a registry (the C# reader's limit).
pub(crate) const MAXIMUM_COUNT: u32 = 100_000;

/// The first `N` bytes as a fixed array, or `None` when fewer are left.
pub(crate) fn le_bytes<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
    bytes.get(..N)?.try_into().ok()
}

/// Whether `start` begins an actor registry: 1 to 16 owners, and the actor id is the first owner.
pub(crate) fn actor_prefix(chunk: &[u8], start: usize, actor_id: u16) -> bool {
    let Some(rest) = chunk.get(start..) else {
        return false;
    };
    let Some(count) = le_bytes::<4>(rest).map(u32::from_le_bytes) else {
        return false;
    };
    let Some(owner) = rest.get(4..).and_then(le_bytes::<2>).map(u16::from_le_bytes) else {
        return false;
    };
    (1..=16).contains(&count) && owner == actor_id
}

/// Latin-1 text of raw bytes: one character per byte, as the C# reader decodes registry strings.
pub(crate) fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

/// Cursor over one registry. Every read returns `None` past the end of the chunk.
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub(crate) const fn new(data: &'a [u8], position: usize) -> Self {
        Self { data, position }
    }

    pub(crate) const fn position(&self) -> usize {
        self.position
    }

    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let end = self.position.checked_add(length)?;
        let bytes = self.data.get(self.position..end)?;
        self.position = end;
        Some(bytes)
    }

    pub(crate) fn skip(&mut self, length: usize) -> Option<()> {
        self.take(length).map(|_| ())
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(le_bytes::<2>(self.take(2)?)?))
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(le_bytes::<4>(self.take(4)?)?))
    }

    pub(crate) fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(le_bytes::<4>(self.take(4)?)?))
    }

    pub(crate) fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(le_bytes::<8>(self.take(8)?)?))
    }

    /// An element count, refused above `MAXIMUM_COUNT`.
    pub(crate) fn count(&mut self) -> Option<usize> {
        let value = self.u32()?;
        if value > MAXIMUM_COUNT {
            return None;
        }
        usize::try_from(value).ok()
    }

    /// Zero-terminated bytes within the first `MAXIMUM_STRING` bytes; control characters are refused.
    pub(crate) fn string(&mut self) -> Option<&'a [u8]> {
        let rest = self.data.get(self.position..)?;
        let window = rest.get(..rest.len().min(MAXIMUM_STRING))?;
        let end = window.iter().position(|byte| *byte == 0)?;
        let bytes = window.get(..end)?;
        if bytes.iter().any(|byte| *byte < 0x20) {
            return None;
        }
        self.position = self.position.checked_add(end)?.checked_add(1)?;
        Some(bytes)
    }

    /// A counted vector of strings; the strings themselves are not kept.
    pub(crate) fn skip_string_vector(&mut self) -> Option<()> {
        for _ in 0..self.count()? {
            self.string()?;
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::Reader;

    #[test]
    fn strings_stop_at_the_terminator_and_refuse_control_characters() {
        let mut reader = Reader::new(b"ab\0\x01\0", 0);
        assert_eq!(reader.string(), Some(&b"ab"[..]));
        assert_eq!(reader.string(), None);
    }

    #[test]
    fn reads_past_the_end_are_none() {
        let mut reader = Reader::new(&[1, 2], 0);
        assert_eq!(reader.u32(), None);
        assert_eq!(reader.u16(), Some(0x0201));
        assert_eq!(reader.u8(), None);
    }
}
