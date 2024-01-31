use std::io::{BufReader, Read};

use zstd::stream::Decoder as ZstdDecoder;

use crate::reader::sync::{Decoder, LimitedReader};

impl<R> Decoder<R> for ZstdDecoder<'static, BufReader<R>>
where
    R: Read,
{
    fn into_inner(self: Box<Self>) -> R {
        Self::finish(*self).into_inner()
    }

    fn get_mut(&mut self) -> &mut R {
        Self::get_mut(self).get_mut()
    }
}

pub(crate) fn mk_decoder(r: LimitedReader) -> std::io::Result<impl Decoder<LimitedReader>> {
    // TODO: have LimitedReader (and Buffer) implement BufRead, cf. https://github.com/fasterthanlime/rc-zip/issues/55
    ZstdDecoder::with_buffer(BufReader::new(r))
}
