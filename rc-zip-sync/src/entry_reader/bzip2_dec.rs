use std::io::Read;

use bzip2::read::BzDecoder;

use crate::decoder::{Decoder, RawEntryReader};

impl<R> Decoder<R> for BzDecoder<R>
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

pub(crate) fn mk_decoder(r: RawEntryReader) -> impl Decoder<RawEntryReader> {
    BzDecoder::new(r)
}
