//! Contains result and error type for FBX reader.

use std::error;
use std::io;
use std::str;
use std::string;
use std::fmt;


pub type Result<T> = ::std::result::Result<T, Error>;

/// Critical parse error.
///
/// This error will be emitted when parsing cannot be continued.
#[derive(Debug)]
pub enum Error {
    /// Conversion from array of u8 to String failed.
    Utf8Error(str::Utf8Error),
    /// Invalid magic binary detected.
    InvalidMagic,
    /// I/O operation error.
    Io(io::Error),
    /// Corrupted or inconsistent FBX data detected.
    DataError(String),
    /// Got an unexpected value, and cannot continue parsing.
    ///
    /// This is specialization of [`DataError`](#variant.DataError).
    UnexpectedValue(String),
    /// Reached unexpected EOF.
    UnexpectedEof,
    /// Attempted to use unimplemented feature.
    Unimplemented(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Utf8Error(ref err) => write!(f, "UTF-8 conversion error: {}", err),
            Error::InvalidMagic => write!(f, "Invalid magic header: Non-FBX or corrupted data?"),
            Error::Io(ref err) => write!(f, "I/O error: {}", err),
            Error::DataError(ref err) => write!(f, "Invalid data: {}", err),
            Error::UnexpectedValue(ref err) => write!(f, "Got an unexpected value: {}", err),
            Error::UnexpectedEof => write!(f, "Unexpected EOF"),
            Error::Unimplemented(ref err) => write!(f, "Unimplemented feature: {}", err),
        }
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match *self {
            Error::Utf8Error(ref err) => err.description(),
            Error::InvalidMagic => "Got an invalid magic header",
            Error::Io(ref err) => err.description(),
            Error::DataError(_) => "Got an invalid data",
            Error::UnexpectedValue(_) => "Invalid value in FBX data",
            Error::UnexpectedEof => "Unexpected EOF",
            Error::Unimplemented(_) => "Attempt to use unimplemented feature",
        }
    }

    fn cause(&self) -> Option<&error::Error> {
        match *self {
            Error::Utf8Error(ref err) => Some(err as &error::Error),
            Error::Io(ref err) => Some(err as &error::Error),
            _ => None,
        }
    }
}

impl Clone for Error {
    fn clone(&self) -> Self {
        use self::Error::*;
        use std::error::Error;
        match *self {
            Utf8Error(ref e) => Utf8Error(e.clone()),
            InvalidMagic => InvalidMagic,
            // `io::Error` (and an error wrapped by `io::Error`) cannot be cloned.
            Io(ref e) => Io(io::Error::new(e.kind(), e.description())),
            DataError(ref e) => DataError(e.clone()),
            UnexpectedValue(ref e) => UnexpectedValue(e.clone()),
            UnexpectedEof => UnexpectedEof,
            Unimplemented(ref e) => Unimplemented(e.clone()),
        }
    }
}

impl From<string::FromUtf8Error> for Error {
    fn from(err: string::FromUtf8Error) -> Error {
        Error::Utf8Error(err.utf8_error())
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        // TODO: `io::Error::UnexpectedEof` should be converted to `Error::UnexpectedEof`.
        Error::Io(err)
    }
}
