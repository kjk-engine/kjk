use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// The first bytes are not this format's signature.
    Signature {
        format: &'static str,
        found: [u8; 8],
    },
    /// A variant of the format this parser does not read.
    Unsupported { format: &'static str, what: String },
    /// The data ends before a table or record it announces.
    Truncated {
        format: &'static str,
        needed: usize,
        len: usize,
    },
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::Signature { format, found } => write!(
                f,
                "{format}: bad signature {:?}",
                String::from_utf8_lossy(found)
            ),
            FormatError::Unsupported { format, what } => write!(f, "{format}: unsupported {what}"),
            FormatError::Truncated {
                format,
                needed,
                len,
            } => write!(f, "{format}: needs {needed} bytes, has {len}"),
        }
    }
}

impl std::error::Error for FormatError {}
