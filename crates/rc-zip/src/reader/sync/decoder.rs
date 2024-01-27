use std::{cmp, io};

pub trait Decoder<R>: io::Read
where
    R: io::Read,
{
    /// Moves the inner reader out of this decoder.
    /// self is boxed because decoders are typically used as trait objects.
    fn into_inner(self: Box<Self>) -> R;

    /// Returns a mutable reference to the inner reader.
    fn get_mut(&mut self) -> &mut R;
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
        self.inner
    }

    fn get_mut(&mut self) -> &mut R {
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

    pub fn into_inner(self) -> circular::Buffer {
        self.inner
    }

    pub fn get_mut(&mut self) -> &mut circular::Buffer {
        &mut self.inner
    }
}

impl io::Read for LimitedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = cmp::min(buf.len() as u64, self.remaining) as usize;
        tracing::trace!(%len, buf_len = buf.len(), remaining = self.remaining, available_data = self.inner.available_data(), available_space = self.inner.available_space(), "computing len");

        let res = self.inner.read(&mut buf[..len]);
        if let Ok(n) = res {
            tracing::trace!(%n, "read ok");
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
