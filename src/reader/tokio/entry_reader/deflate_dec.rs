use async_compression::tokio::bufread::DeflateDecoder;
use tokio::io::AsyncBufRead;

use crate::reader::{tokio::decoder::AsyncDecoder, RawEntryReader};

impl<R> AsyncDecoder<R> for DeflateDecoder<R>
where
    R: AsyncBufRead,
{
    fn into_inner(self: Box<Self>) -> R {
        Self::into_inner(*self)
    }

    fn get_mut(&mut self) -> &mut R {
        Self::get_mut(self)
    }
}

pub(crate) fn mk_decoder(r: RawEntryReader) -> impl AsyncDecoder<RawEntryReader> {
    DeflateDecoder::new(r)
}
