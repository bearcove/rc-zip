use winnow::{error::ErrorKind, IResult};

/// Result of a parse operation
///
/// Used internally when parsing, for example, the end of central directory record.
pub type Result<'a, T> = IResult<&'a [u8], T, Error<'a>>;

/// Parsing error, see [Error].
pub type Error<'a> = (&'a [u8], ErrorKind);
