use std::fmt;
use std::io;

#[derive(Debug)]
pub enum Error {
    InvalidArgument(String),
    InvalidDictionary(String),
    Io(io::Error),
    NotFound(String),
    OutOfRange(String),
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::InvalidArgument(message.into())
    }

    pub fn invalid_dictionary(message: impl Into<String>) -> Self {
        Self::InvalidDictionary(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArgument(message) => f.write_str(message),
            Self::InvalidDictionary(message) => f.write_str(message),
            Self::Io(err) => err.fmt(f),
            Self::NotFound(message) => f.write_str(message),
            Self::OutOfRange(message) => f.write_str(message),
            Self::Unsupported(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
