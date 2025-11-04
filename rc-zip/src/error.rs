//! All error types used in this crate

use crate::parse::Method;

use super::encoding;

/// Any zip-related error, from invalid archives to encoding problems.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Not a valid zip file, or a variant that is unsupported.
    #[error("format: {0}")]
    Format(#[from] FormatError),

    /// Something is not supported by this crate
    #[error("unsupported: {0}")]
    Unsupported(#[from] UnsupportedError),

    /// Invalid UTF-8, Shift-JIS, or any problem encountered while decoding text in general.
    #[error("encoding: {0:?}")]
    Encoding(#[from] encoding::DecodingError),

    /// I/O-related error
    #[error("io: {0}")]
    IO(#[from] std::io::Error),

    /// Decompression-related error
    #[error("{method:?} decompression error: {msg}")]
    Decompression {
        /// The compression method that failed
        method: Method,
        /// Additional information
        msg: String,
    },

    /// Could not read as a zip because size could not be determined
    #[error("size must be known to open zip file")]
    UnknownSize,
}

impl Error {
    /// Create a new error indicating that the given method is not supported.
    pub fn method_not_supported(method: Method) -> Self {
        Self::Unsupported(UnsupportedError::MethodNotSupported(method))
    }

    /// Create a new error indicating that the given method is not enabled.
    pub fn method_not_enabled(method: Method) -> Self {
        Self::Unsupported(UnsupportedError::MethodNotEnabled(method))
    }
}

/// Some part of the zip format is not supported by this crate.
#[derive(Debug, thiserror::Error)]
pub enum UnsupportedError {
    /// The compression method is not supported.
    #[error("compression method not supported: {0:?}")]
    MethodNotSupported(Method),

    /// The compression method is supported, but not enabled in this build.
    #[error("compression method supported, but not enabled in this build: {0:?}")]
    MethodNotEnabled(Method),

    /// The zip file uses a version of LZMA that is not supported.
    #[error("only LZMA2.0 is supported, found LZMA{minor}.{major}")]
    LzmaVersionUnsupported {
        /// major version read from LZMA properties header, cf. appnote 5.8.8
        major: u8,
        /// minor version read from LZMA properties header, cf. appnote 5.8.8
        minor: u8,
    },

    /// The LZMA properties header is not the expected size.
    #[error("LZMA properties header wrong size: expected {expected} bytes, got {actual} bytes")]
    LzmaPropertiesHeaderWrongSize {
        /// expected size in bytes
        expected: u16,
        /// actual size in bytes, read from a u16, cf. appnote 5.8.8
        actual: u16,
    },
}

/// Specific zip format errors, mostly due to invalid zip archives but that could also stem from
/// implementation shortcomings.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    /// The end of central directory record was not found.
    ///
    /// This usually indicates that the file being read is not a zip archive.
    #[error("end of central directory record not found")]
    DirectoryEndSignatureNotFound,

    /// The zip64 end of central directory record could not be parsed.
    ///
    /// This is only returned when a zip64 end of central directory *locator* was found,
    /// so the archive should be zip64, but isn't.
    #[error("zip64 end of central directory record not found")]
    Directory64EndRecordInvalid,

    /// Corrupted/partial zip file: the offset we found for the central directory
    /// points outside of the current file.
    #[error("directory offset points outside of file")]
    DirectoryOffsetPointsOutsideFile,

    /// The central record is corrupted somewhat.
    ///
    /// This can happen when the end of central directory record advertises
    /// a certain number of files, but we weren't able to read the same number of central directory
    /// headers.
    #[error("invalid central record: expected to read {expected} files, got {actual}")]
    InvalidCentralRecord {
        /// expected number of files
        expected: u16,
        /// actual number of files
        actual: u16,
    },

    /// An extra field (that we support) was not decoded correctly.
    ///
    /// This can indicate an invalid zip archive, or an implementation error in this crate.
    #[error("could not decode extra field")]
    InvalidExtraField,

    /// The header offset of an entry is invalid.
    ///
    /// This can indicate an invalid zip archive, or an invalid user-provided global offset
    #[error("invalid header offset")]
    InvalidHeaderOffset,

    /// End of central directory record claims an impossible number of files.
    ///
    /// Each entry takes a minimum amount of size, so if the overall archive size is smaller than
    /// claimed_records_count * minimum_entry_size, we know it's not a valid zip file.
    #[error("impossible number of files: claims to have {claimed_records_count}, but zip size is {zip_size}")]
    ImpossibleNumberOfFiles {
        /// number of files claimed in the end of central directory record
        claimed_records_count: u64,
        /// total size of the zip file
        zip_size: u64,
    },

    /// The local file header (before the file data) could not be parsed correctly.
    #[error("invalid local file header")]
    InvalidLocalHeader,

    /// The data descriptor (after the file data) could not be parsed correctly.
    #[error("invalid data descriptor")]
    InvalidDataDescriptor,

    /// The uncompressed size didn't match
    #[error("uncompressed size didn't match: expected {expected}, got {actual}")]
    WrongSize {
        /// expected size in bytes (from the local header, data descriptor, etc.)
        expected: u64,
        /// actual size in bytes (from decompressing the entry)
        actual: u64,
    },

    /// The CRC-32 checksum didn't match.
    #[error("checksum didn't match: expected {expected:x?}, got {actual:x?}")]
    WrongChecksum {
        /// expected checksum (from the data descriptor, etc.)
        expected: u32,
        /// actual checksum (from decompressing the entry)
        actual: u32,
    },
}

impl From<Error> for std::io::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::IO(e) => e,
            e => std::io::Error::other(e),
        }
    }
}
