use std::io::{BufRead, Read};

use zstd::stream::Decoder as ZstdDecoder;

use crate::reader::sync::{Decoder, RawEntryReader};

impl<R> Decoder<R> for ZstdDecoder<'static, R>
where
    R: Read + BufRead,
{
    fn into_inner(self: Box<Self>) -> R {
        Self::finish(*self)
    }

    fn get_mut(&mut self) -> &mut R {
        Self::get_mut(self)
    }
}

pub(crate) fn mk_decoder(r: RawEntryReader) -> std::io::Result<impl Decoder<RawEntryReader>> {
    tracing::trace!("Creating ZstdDecoder");
    ZstdDecoder::with_buffer(r)
}
