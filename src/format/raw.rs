use crate::format::*;
use hex_fmt::HexFmt;
use nom::{bytes::streaming::take, combinator::map};
use std::fmt;

/// A raw zip string, with no specific encoding.
///
/// This is used while parsing a zip archive's central directory,
/// before we know what encoding is used.
#[derive(Clone)]
pub struct ZipString(pub Vec<u8>);

impl<'a> From<&'a [u8]> for ZipString {
    fn from(slice: &'a [u8]) -> Self {
        Self(slice.into())
    }
}

impl fmt::Debug for ZipString {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match std::str::from_utf8(&self.0) {
            Ok(s) => write!(f, "{:?}", s),
            Err(_) => write!(f, "[non-utf8 string: {:x}]", HexFmt(&self.0)),
        }
    }
}

impl ZipString {
    pub(crate) fn parser<'a, C>(count: C) -> impl Fn(&'a [u8]) -> parse::Result<'a, Self>
    where
        C: nom::ToUsize,
    {
        map(take(count.to_usize()), |slice: &'a [u8]| {
            ZipString(slice.into())
        })
    }

    pub(crate) fn as_option(self) -> Option<ZipString> {
        if self.0.len() > 0 {
            Some(self)
        } else {
            None
        }
    }
}

/// A raw u8 slice, with no specific structure.
///
/// This is used while parsing a zip archive, when we want
/// to retain an owned slice to be parsed later.
#[derive(Clone)]
pub struct ZipBytes(pub Vec<u8>);

impl fmt::Debug for ZipBytes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        const MAX_SHOWN_SIZE: usize = 10;
        let data = &self.0[..];
        let (slice, extra) = if data.len() > MAX_SHOWN_SIZE {
            (&self.0[..MAX_SHOWN_SIZE], Some(data.len() - MAX_SHOWN_SIZE))
        } else {
            (&self.0[..], None)
        };
        write!(f, "{:x}", HexFmt(slice))?;
        if let Some(extra) = extra {
            write!(f, " (+ {} bytes)", extra)?;
        }
        Ok(())
    }
}

impl ZipBytes {
    pub(crate) fn parser<'a, C>(count: C) -> impl Fn(&'a [u8]) -> parse::Result<'a, Self>
    where
        C: nom::ToUsize,
    {
        map(take(count.to_usize()), |slice: &'a [u8]| {
            ZipBytes(slice.into())
        })
    }
}
