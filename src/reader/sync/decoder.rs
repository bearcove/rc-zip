use std::{cmp, io};

use oval::Buffer;

use crate::reader::RawEntryReader;

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

impl io::BufRead for RawEntryReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        let max_avail = cmp::min(self.remaining, self.inner.available_data() as u64);
        Ok(&self.inner.data()[..max_avail as usize])
    }

    fn consume(&mut self, amt: usize) {
        self.remaining -= amt as u64;
        Buffer::consume(&mut self.inner, amt);
    }
}

impl io::Read for RawEntryReader {
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
