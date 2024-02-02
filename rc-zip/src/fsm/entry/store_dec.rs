use std::cmp;

use super::{DecompressOutcome, Decompressor};

#[derive(Default)]
pub(crate) struct StoreDec;

impl Decompressor for StoreDec {
    fn decompress(
        &mut self,
        in_buf: &[u8],
        out_buf: &mut [u8],
    ) -> Result<DecompressOutcome, crate::error::Error> {
        let len = cmp::min(in_buf.len(), out_buf.len());
        out_buf[..len].copy_from_slice(&in_buf[..len]);
        Ok(DecompressOutcome {
            bytes_read: len,
            bytes_written: len,
        })
    }
}
