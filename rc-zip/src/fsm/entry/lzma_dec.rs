use crate::{error::Error, parse::Method};

use super::{DecompressOutcome, Decompressor, HasMoreInput};

pub(crate) struct LzmaDec {}

impl Default for LzmaDec {
    fn default() -> Self {
        Self {}
    }
}

impl Decompressor for LzmaDec {
    fn decompress(
        &mut self,
        in_buf: &[u8],
        out: &mut [u8],
        _has_more_input: HasMoreInput,
    ) -> Result<DecompressOutcome, Error> {
        todo!()
    }
}
