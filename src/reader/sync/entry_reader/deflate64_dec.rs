use std::io::{BufReader, Read};

use deflate64::Deflate64Decoder;

use crate::reader::sync::{Decoder, LimitedReader};

impl<R> Decoder<R> for Deflate64Decoder<BufReader<R>>
where
    R: Read,
{
    fn into_inner(self: Box<Self>) -> R {
        Self::into_inner(*self).into_inner()
    }

    fn get_mut(&mut self) -> &mut R {
        Self::get_mut(self).get_mut()
    }
}

pub(crate) fn mk_decoder(r: LimitedReader) -> impl Decoder<LimitedReader> {
    Deflate64Decoder::new(r)
}
