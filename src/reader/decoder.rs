use libflate::non_blocking::deflate;
use std::{cmp, io};

pub trait Decoder<R>: io::Read
where
    R: io::Read,
{
    /// Moves the inner reader out of this decoder.
    /// self is boxed because decoders are typically used as trait objects.
    fn into_inner(self: Box<Self>) -> R;
    /// Returns a mutable reference to the inner reader.
    fn as_inner_mut<'a>(&'a mut self) -> &'a mut R;
}

impl<R> Decoder<R> for deflate::Decoder<R>
where
    R: io::Read,
{
    fn into_inner(self: Box<Self>) -> R {
        deflate::Decoder::into_inner(*self)
    }

    fn as_inner_mut<'a>(&'a mut self) -> &'a mut R {
        deflate::Decoder::as_inner_mut(self)
    }
}

pub struct StoreDecoder<R>
where
    R: io::Read,
{
    inner: R,
}

impl<R> StoreDecoder<R>
where
    R: io::Read,
{
    pub fn new(inner: R) -> Self {
        Self { inner }
    }
}

impl<R> io::Read for StoreDecoder<R>
where
    R: io::Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<R> Decoder<R> for StoreDecoder<R>
where
    R: io::Read,
{
    fn into_inner(self: Box<Self>) -> R {
        (*self).inner
    }

    fn as_inner_mut<'a>(&'a mut self) -> &'a mut R {
        &mut self.inner
    }
}

/// Only allows reading a fixed number of bytes from a [circular::Buffer],
/// allowing to move the inner reader out afterwards.
pub struct LimitedReader {
    remaining: u64,
    inner: circular::Buffer,
}

impl LimitedReader {
    pub fn new(inner: circular::Buffer, remaining: u64) -> Self {
        Self { inner, remaining }
    }

    pub fn into_inner(self: Self) -> circular::Buffer {
        self.inner
    }

    pub fn as_inner_mut<'a>(&'a mut self) -> &'a mut circular::Buffer {
        &mut self.inner
    }
}

impl io::Read for LimitedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.inner.available_space() == 0 {
            self.inner.shift();
        }

        let len = cmp::min(buf.len() as u64, self.remaining) as usize;
        let res = self.inner.read(&mut buf[..len]);
        if let Ok(n) = res {
            self.remaining -= n as u64;
        }
        res
    }
}

/// Normalize EOF behavior for std::fs::File on Windows
/// (ie. makes it return Ok(0), not an OS-level I/O error)
/// Works for non-file Read impls, on non-Windows OSes too.
pub struct EOFNormalizer<R>
where
    R: io::Read,
{
    inner: R,
}

impl<R> EOFNormalizer<R>
where
    R: io::Read,
{
    pub fn new(inner: R) -> Self {
        Self { inner }
    }
}

impl<R> io::Read for EOFNormalizer<R>
where
    R: io::Read,
{
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        #[cfg(windows)]
        match self.inner.read(buf) {
            Err(e) => match e.raw_os_error() {
                // Windows error 38 = Reached end of file
                Some(38) => Ok(0),
                _ => Err(e),
            },
            x => x,
        }

        #[cfg(not(windows))]
        self.inner.read(buf)
    }
}
