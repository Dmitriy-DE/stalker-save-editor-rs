use crate::{Error, Result};

/// A forward reader over borrowed bytes. Every read is bounds-checked and every position is computed with checked
/// arithmetic: a damaged or hostile save produces [`Error::Damaged`], never a panic and never a read elsewhere.
#[derive(Debug, Clone)]
pub struct Cursor<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    /// Starts at the first byte.
    #[must_use]
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    /// Bytes consumed so far.
    #[must_use]
    pub fn position(&self) -> usize {
        self.position
    }

    /// Bytes left.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.position)
    }

    /// The next `length` bytes, borrowed from the source.
    ///
    /// # Errors
    /// [`Error::Damaged`] when fewer than `length` bytes are left.
    pub fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self.position.checked_add(length).ok_or_else(|| self.short(length))?;
        let slice = self.data.get(self.position..end).ok_or_else(|| self.short(length))?;
        self.position = end;
        Ok(slice)
    }

    /// Skips `length` bytes.
    ///
    /// # Errors
    /// [`Error::Damaged`] when fewer than `length` bytes are left.
    pub fn skip(&mut self, length: usize) -> Result<()> {
        self.take(length).map(|_| ())
    }

    /// One byte.
    ///
    /// # Errors
    /// [`Error::Damaged`] at the end of the data.
    pub fn u8(&mut self) -> Result<u8> {
        Ok(u8::from_le_bytes(self.array()?))
    }

    /// Little-endian `u16`.
    ///
    /// # Errors
    /// [`Error::Damaged`] when the data ends first.
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    /// Little-endian `u32`.
    ///
    /// # Errors
    /// [`Error::Damaged`] when the data ends first.
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// Little-endian `u64`.
    ///
    /// # Errors
    /// [`Error::Damaged`] when the data ends first.
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    /// Little-endian `f32`.
    ///
    /// # Errors
    /// [`Error::Damaged`] when the data ends first.
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    /// Bytes up to (not including) the next zero byte; the zero is consumed. Limited to `maximum` bytes.
    ///
    /// # Errors
    /// [`Error::Damaged`] when no zero byte follows within `maximum` bytes.
    pub fn zero_terminated(&mut self, maximum: usize) -> Result<&'a [u8]> {
        let rest = self.data.get(self.position..).unwrap_or_default();
        let window = rest.get(..maximum.min(rest.len())).unwrap_or_default();
        let length = window.iter().position(|byte| *byte == 0).ok_or_else(|| {
            Error::damaged(format!(
                "no end of string within {maximum} bytes at offset {}",
                self.position
            ))
        })?;
        let text = self.take(length)?;
        self.skip(1)?;
        Ok(text)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let slice = self.take(N)?;
        <[u8; N]>::try_from(slice).map_err(|_| self.short(N))
    }

    fn short(&self, wanted: usize) -> Error {
        Error::damaged(format!(
            "{wanted} bytes wanted at offset {}, {} left",
            self.position,
            self.remaining()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::Cursor;
    use crate::Error;

    #[test]
    fn reads_little_endian_values_and_strings_in_order() {
        let data = [0x34, 0x12, 0x78, 0x56, 0x34, 0x12, b'o', b'k', 0, 0xFF];
        let mut cursor = Cursor::new(&data);
        assert_eq!(cursor.u16(), Ok(0x1234));
        assert_eq!(cursor.u32(), Ok(0x1234_5678));
        assert_eq!(cursor.zero_terminated(16), Ok(&b"ok"[..]));
        assert_eq!(cursor.u8(), Ok(0xFF));
        assert_eq!(cursor.remaining(), 0);
    }

    #[test]
    fn a_read_past_the_end_is_an_error_and_moves_nothing() {
        let mut cursor = Cursor::new(&[1, 2, 3]);
        assert!(matches!(cursor.u32(), Err(Error::Damaged(_))));
        assert_eq!(cursor.position(), 0);
        assert!(matches!(cursor.take(usize::MAX), Err(Error::Damaged(_))));
        assert_eq!(cursor.u8(), Ok(1));
    }

    #[test]
    fn a_string_without_an_end_is_refused() {
        let mut cursor = Cursor::new(b"abcdef");
        assert!(matches!(cursor.zero_terminated(4), Err(Error::Damaged(_))));
        assert!(matches!(cursor.zero_terminated(64), Err(Error::Damaged(_))));
    }
}
