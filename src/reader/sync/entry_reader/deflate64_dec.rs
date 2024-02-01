use std::io::{BufReader, Read};

use deflate64::Deflate64Decoder;

use crate::reader::{sync::decoder::Decoder, RawEntryReader};

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

pub(crate) fn mk_decoder(r: RawEntryReader) -> impl Decoder<RawEntryReader> {
    Deflate64Decoder::new(r)
}
