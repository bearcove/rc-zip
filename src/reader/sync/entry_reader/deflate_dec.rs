use std::io::Read;

use flate2::read::DeflateDecoder;

use crate::reader::sync::{Decoder, LimitedReader};

impl<R> Decoder<R> for DeflateDecoder<R>
where
    R: Read,
{
    fn into_inner(self: Box<Self>) -> R {
        Self::into_inner(*self)
    }

    fn get_mut(&mut self) -> &mut R {
        Self::get_mut(self)
    }
}

pub(crate) fn mk_decoder(r: LimitedReader) -> impl Decoder<LimitedReader> {
    DeflateDecoder::new(r)
}
