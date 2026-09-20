use std::fmt;

/// Errors produced by the H.265 decoder.
#[derive(Debug)]
pub enum DecodeError {
    /// Bitstream ended unexpectedly during parsing.
    UnexpectedEof,
    /// A syntactic element in the bitstream has an invalid value.
    InvalidSyntax(&'static str),
    /// The stream uses a feature this decoder doesn't support yet.
    Unsupported(&'static str),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::UnexpectedEof => write!(f, "unexpected end of bitstream"),
            DecodeError::InvalidSyntax(msg) => write!(f, "invalid syntax: {}", msg),
            DecodeError::Unsupported(msg) => write!(f, "unsupported: {}", msg),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Allow `?` to convert `&'static str` errors from internal parsing functions
/// into `DecodeError` automatically.
impl From<&'static str> for DecodeError {
    fn from(msg: &'static str) -> Self {
        if msg == "end of bitstream" {
            DecodeError::UnexpectedEof
        } else if msg.contains("not yet supported")
            || msg.contains("not supported")
            || msg.starts_with("only ")
            || msg.starts_with("unsupported")
        {
            DecodeError::Unsupported(msg)
        } else {
            DecodeError::InvalidSyntax(msg)
        }
    }
}
