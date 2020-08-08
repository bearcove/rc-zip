use async_compression::futures::bufread::DeflateDecoder;
use futures::{AsyncBufRead, AsyncRead};

pub trait AsyncDecoder<R>: AsyncRead + Unpin
where
    R: AsyncRead,
{
    fn into_inner(self) -> R;
}

impl<R> AsyncDecoder<R> for DeflateDecoder<R>
where
    R: AsyncRead + AsyncBufRead + Unpin,
{
    fn into_inner(self) -> R {
        DeflateDecoder::into_inner(self)
    }
}

impl<R> AsyncDecoder<R> for R
where
    R: AsyncRead + Unpin,
{
    fn into_inner(self) -> R {
        self
    }
}
