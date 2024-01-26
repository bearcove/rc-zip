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

    /// Could not read as a zip because size could not be determined
    #[error("size must be known to open zip file")]
    UnknownSize,
}

#[derive(Debug, thiserror::Error)]
pub enum UnsupportedError {
    #[error("unsupported compression method: {0:?}")]
    UnsupportedCompressionMethod(crate::format::Method),
    #[error("compression method supported, but not enabled in this build: {0:?}")]
    CompressionMethodNotEnabled(crate::format::Method),
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
    InvalidCentralRecord { expected: u16, actual: u16 },

    /// An extra field (that we support) was not decoded correctly.
    ///
    /// This can indicate an invalid zip archive, or an implementation error in this crate.
    #[error("could not decode extra field")]
    InvalidExtraField,

    /// End of central directory record claims an impossible number of file.
    ///
    /// Each entry takes a minimum amount of size, so if the overall archive size is smaller than
    /// claimed_records_count * minimum_entry_size, we know it's not a valid zip file.
    #[error("impossible number of files: claims to have {claimed_records_count}, but zip size is {zip_size}")]
    ImpossibleNumberOfFiles {
        claimed_records_count: u64,
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
    WrongSize { expected: u64, actual: u64 },

    /// The CRC-32 checksum didn't match.
    #[error("checksum didn't match: expected {expected:x?}, got {actual:x?}")]
    WrongChecksum { expected: u32, actual: u32 },
}

impl From<Error> for std::io::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::IO(e) => e,
            e => std::io::Error::new(std::io::ErrorKind::Other, e),
        }
    }
}
