use std::{error, fmt};

pub type DecodingError<'a> = (&'a [u8], nom::error::ErrorKind);

#[derive(Debug)]
pub enum Error {
    IO(std::io::Error),
    Decoding(nom::error::ErrorKind),
    Format(FormatError),
    String(String),
}

#[derive(Debug)]
pub enum FormatError {
    DirectoryEndSignatureNotFound,
    DirectoryOffsetPointsOutsideFile,
    ImpossibleNumberOfFiles {
        claimed_records_count: u64,
        zip_size: u64,
    },
}

impl error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::IO(e) => write!(f, "rc-zip IO error: {}", e),
            Error::Decoding(e) => write!(f, "rc-zip decoding error: {:#?}", e),
            Error::Format(e) => write!(f, "rc-zip error: invalid zip file: {:#?}", e),
            Error::String(e) => write!(f, "rc-zip error: {}", e),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IO(e)
    }
}

impl From<FormatError> for Error {
    fn from(e: FormatError) -> Self {
        Error::Format(e)
    }
}

impl<'a> From<DecodingError<'a>> for Error {
    fn from(e: DecodingError<'a>) -> Self {
        Error::Decoding(e.1)
    }
}
