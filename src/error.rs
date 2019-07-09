use std::{error, fmt};

pub(crate) type DecodingError<'a> = (&'a [u8], nom::error::ErrorKind);

#[derive(Debug)]
pub enum Error<'a> {
    IO(std::io::Error),
    Decoding(DecodingError<'a>),
    Format(FormatError),
    String(String),
}

#[derive(Debug)]
pub enum FormatError {
    DirectoryEndSignatureNotFound,
    DirectoryOffsetPointsOutsideFile,
}

impl<'a> error::Error for Error<'a> {}

impl<'a> fmt::Display for Error<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::IO(e) => write!(f, "rc-zip IO error: {}", e),
            Error::Decoding(e) => write!(f, "rc-zip decoding error: {:#?}", e),
            Error::Format(e) => write!(f, "rc-zip error: invalid zip file: {:#?}", e),
            Error::String(e) => write!(f, "rc-zip error: {}", e),
        }
    }
}

impl<'a> From<std::io::Error> for Error<'a> {
    fn from(e: std::io::Error) -> Self {
        Error::IO(e)
    }
}

impl<'a> From<FormatError> for Error<'a> {
    fn from(e: FormatError) -> Self {
        Error::Format(e)
    }
}

impl<'a> From<DecodingError<'a>> for Error<'a> {
    fn from(e: DecodingError<'a>) -> Self {
        Error::Decoding(e)
    }
}
