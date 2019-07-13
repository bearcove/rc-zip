use super::encoding;
use std::{error, fmt};

pub type ZipParseError<'a> = (&'a [u8], nom::error::ErrorKind);

#[derive(Debug)]
pub enum Error {
    IO(std::io::Error),
    Parsing(nom::error::ErrorKind),
    Encoding(encoding::DecodingError),
    Format(FormatError),
    Unimplemented,
}

#[derive(Debug)]
pub enum FormatError {
    DirectoryEndSignatureNotFound,
    Directory64EndRecordInvalid,
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
            Error::Parsing(e) => write!(f, "rc-zip parse error: {:#?}", e),
            Error::Encoding(e) => write!(f, "rc-zip encoding error: {:#?}", e),
            Error::Format(e) => write!(f, "rc-zip error: invalid zip file: {:#?}", e),
            Error::Unimplemented => write!(f, "rc-zip error: unimplemented"),
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

impl<'a> From<ZipParseError<'a>> for Error {
    fn from(e: ZipParseError<'a>) -> Self {
        Error::Parsing(e.1)
    }
}

impl From<encoding::DecodingError> for Error {
    fn from(e: encoding::DecodingError) -> Self {
        Error::Encoding(e)
    }
}
