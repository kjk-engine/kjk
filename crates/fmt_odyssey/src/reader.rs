use crate::FormatError;

/// Little-endian reads at explicit offsets, failing as `Truncated` instead of
/// panicking.
pub(crate) struct Reader<'a> {
    format: &'static str,
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(format: &'static str, bytes: &'a [u8]) -> Self {
        Reader { format, bytes }
    }

    pub fn slice(&self, at: usize, len: usize) -> Result<&'a [u8], FormatError> {
        let end = at.checked_add(len).ok_or(self.truncated(usize::MAX))?;
        self.bytes.get(at..end).ok_or(self.truncated(end))
    }

    pub fn u16(&self, at: usize) -> Result<u16, FormatError> {
        let bytes = self.slice(at, 2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub fn u32(&self, at: usize) -> Result<u32, FormatError> {
        let bytes = self.slice(at, 4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// The 8-byte signature and version at the start of the file.
    pub fn expect_tag(&self, tag: &[u8; 8]) -> Result<(), FormatError> {
        let found = self.slice(0, 8)?;
        if found != tag {
            return Err(FormatError::Signature {
                format: self.format,
                found: found.try_into().expect("slice of 8"),
            });
        }
        Ok(())
    }

    /// A table of `count` records of `size` bytes at `at`, checked to fit.
    pub fn table(&self, at: usize, count: usize, size: usize) -> Result<(), FormatError> {
        let len = count.checked_mul(size).ok_or(self.truncated(usize::MAX))?;
        self.slice(at, len).map(|_| ())
    }

    fn truncated(&self, needed: usize) -> FormatError {
        FormatError::Truncated {
            format: self.format,
            needed,
            len: self.bytes.len(),
        }
    }
}
