use std::{cmp, io::Write};

use crate::{error::Error, parse::Method};

use super::{DecompressOutcome, Decompressor, HasMoreInput};

use lzma_rs::decompress::Stream;
use tracing::trace;

pub(crate) struct LzmaDec {
    stream: Stream<Vec<u8>>,
}

impl LzmaDec {
    pub fn new(uncompressed_size: u64) -> Self {
        trace!(%uncompressed_size, "LzmaDec::new");
        let memlimit = 128 * 1024 * 1024;
        let opts = lzma_rs::decompress::Options {
            unpacked_size: lzma_rs::decompress::UnpackedSize::UseProvided(Some(uncompressed_size)),
            allow_incomplete: false,
            memlimit: Some(memlimit),
        };

        Self {
            stream: Stream::new_with_options(&opts, vec![]),
        }
    }
}

impl Decompressor for LzmaDec {
    fn decompress(
        &mut self,
        in_buf: &[u8],
        out: &mut [u8],
        _has_more_input: HasMoreInput,
    ) -> Result<DecompressOutcome, Error> {
        tracing::trace!(
            in_buf_len = in_buf.len(),
            out_len = out.len(),
            remain_in_internal_buffer = self.stream.get_output_mut().unwrap().len(),
            "DeflateDec::decompress",
        );

        let mut outcome: DecompressOutcome = Default::default();

        self.copy_to_out(out, &mut outcome);
        if outcome.bytes_written > 0 {
            trace!("LzmaDec: bytes_written > 0");
            return Ok(outcome);
        }

        let n = self
            .stream
            .write(in_buf)
            .map_err(|e| Error::Decompression {
                method: Method::Lzma,
                msg: e.to_string(),
            })?;
        trace!("LzmaDec: wrote n = {}", n);
        outcome.bytes_read = n;

        self.copy_to_out(out, &mut outcome);
        trace!("LzmaDec: bytes_written = {}", outcome.bytes_written);
        Ok(outcome)
    }
}

impl LzmaDec {
    fn copy_to_out(&mut self, mut out: &mut [u8], outcome: &mut DecompressOutcome) {
        let internal_buf = self.stream.get_output_mut().unwrap();

        while !out.is_empty() && !internal_buf.is_empty() {
            let to_copy = cmp::min(out.len(), internal_buf.len());
            trace!("LzmaDec: to_copy = {}", to_copy);
            out[..to_copy].copy_from_slice(&internal_buf[..to_copy]);
            out = &mut out[to_copy..];

            // rotate the internal buffer
            internal_buf.rotate_left(to_copy);
            // and shrink it
            internal_buf.resize(internal_buf.len() - to_copy, 0);

            outcome.bytes_written += to_copy;
        }
    }
}
