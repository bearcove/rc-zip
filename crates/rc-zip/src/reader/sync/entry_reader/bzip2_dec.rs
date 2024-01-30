use std::io::Read;

use bzip2::read::BzDecoder;

use crate::reader::sync::{Decoder, LimitedReader};

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

pub(crate) fn mk_decoder(r: LimitedReader) -> impl Decoder<LimitedReader> {
    BzDecoder::new(r)
}
