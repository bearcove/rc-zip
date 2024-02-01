use std::{cmp, io, pin::Pin, task};

use oval::Buffer;
use tokio::io::{AsyncBufRead, AsyncRead};

use crate::reader::RawEntryReader;

pub(crate) trait AsyncDecoder<R>: AsyncRead
where
    R: AsyncRead,
{
    /// Moves the inner reader out of this decoder.
    /// self is boxed because decoders are typically used as trait objects.
    fn into_inner(self: Box<Self>) -> R;

    /// Returns a mutable reference to the inner reader.
    fn get_mut(&mut self) -> &mut R;
}

pin_project_lite::pin_project! {
    pub(crate) struct StoreAsyncDecoder<R>
    where
        R: AsyncRead,
    {
        #[pin]
        inner: R,
    }
}

impl<R> StoreAsyncDecoder<R>
where
    R: AsyncRead,
{
    pub(crate) fn new(inner: R) -> Self {
        Self { inner }
    }
}

impl<R> AsyncRead for StoreAsyncDecoder<R>
where
    R: AsyncRead,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> task::Poll<io::Result<()>> {
        let this = self.project();
        this.inner.poll_read(cx, buf)
    }
}

impl<R> AsyncDecoder<R> for StoreAsyncDecoder<R>
where
    R: AsyncRead,
{
    fn into_inner(self: Box<Self>) -> R {
        self.inner
    }

    fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }
}

impl AsyncBufRead for RawEntryReader {
    fn consume(mut self: Pin<&mut Self>, amt: usize) {
        self.as_mut().remaining -= amt as u64;
        Buffer::consume(&mut self.inner, amt);
    }

    fn poll_fill_buf(
        self: Pin<&mut Self>,
        _cx: &mut task::Context<'_>,
    ) -> task::Poll<io::Result<&[u8]>> {
        let max_avail = cmp::min(self.remaining, self.inner.available_data() as u64);
        Ok(self.get_mut().inner.data()[..max_avail as _].as_ref()).into()
    }
}

impl AsyncRead for RawEntryReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> task::Poll<io::Result<()>> {
        let len = cmp::min(
            buf.remaining() as u64,
            cmp::min(self.remaining, self.inner.available_data() as _),
        ) as usize;
        tracing::trace!(%len, buf_remaining = buf.remaining(), remaining = self.remaining, available_data = self.inner.available_data(), available_space = self.inner.available_space(), "computing len");

        buf.put_slice(&self.inner.data()[..len]);
        self.as_mut().inner.consume(len);
        self.remaining -= len as u64;

        Ok(()).into()
    }
}
