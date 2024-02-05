use pretty_hex::PrettyHex;
use std::fmt;
use winnow::{stream::ToUsize, token::take, PResult, Parser, Partial};

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
            Err(_) => write!(f, "[non-utf8 string: {}]", self.0.hex_dump()),
        }
    }
}

impl ZipString {
    pub(crate) fn parser<C>(count: C) -> impl FnMut(&mut Partial<&'_ [u8]>) -> PResult<Self>
    where
        C: ToUsize,
    {
        let count = count.to_usize();
        move |i| (take(count).map(|slice: &[u8]| Self(slice.into()))).parse_next(i)
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
        write!(f, "{}", slice.hex_dump())?;
        if let Some(extra) = extra {
            write!(f, " (+ {} bytes)", extra)?;
        }
        Ok(())
    }
}

impl ZipBytes {
    pub(crate) fn parser<C>(count: C) -> impl FnMut(&mut Partial<&'_ [u8]>) -> PResult<Self>
    where
        C: ToUsize,
    {
        let count = count.to_usize();
        move |i| (take(count).map(|slice: &[u8]| Self(slice.into()))).parse_next(i)
    }
}
