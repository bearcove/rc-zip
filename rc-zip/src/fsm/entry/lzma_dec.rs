use std::{cmp, io::Write};

use crate::{error::Error, parse::Method};

use super::{DecompressOutcome, Decompressor, HasMoreInput};

use lzma_rs::decompress::{Options, Stream, UnpackedSize};
use tracing::trace;

#[derive(Default)]
enum State {
    Writing(Box<Stream<Vec<u8>>>),
    Draining(Vec<u8>),

    #[default]
    Transition,
}

pub(crate) struct LzmaDec {
    state: State,
}

impl LzmaDec {
    pub fn new(uncompressed_size: u64) -> Self {
        let stream = Stream::new_with_options(
            &(Options {
                unpacked_size: UnpackedSize::UseProvided(Some(uncompressed_size)),
                allow_incomplete: false,
                memlimit: Some(128 * 1024 * 1024),
            }),
            vec![],
        );

        Self {
            state: State::Writing(Box::new(stream)),
        }
    }
}

impl Decompressor for LzmaDec {
    fn decompress(
        &mut self,
        in_buf: &[u8],
        out: &mut [u8],
        has_more_input: HasMoreInput,
    ) -> Result<DecompressOutcome, Error> {
        tracing::trace!(
            in_buf_len = in_buf.len(),
            out_len = out.len(),
            remain_in_internal_buffer = self.internal_buf_mut().len(),
            "decompress",
        );

        let mut outcome: DecompressOutcome = Default::default();

        self.copy_to_out(out, &mut outcome);
        if outcome.bytes_written > 0 {
            trace!(
                "still draining internal buffer, just copied {} bytes",
                outcome.bytes_written
            );
            return Ok(outcome);
        }

        match &mut self.state {
            State::Writing(stream) => {
                let n = stream.write(in_buf).map_err(dec_err)?;
                trace!(
                    "wrote {} bytes to decompressor (of {} available)",
                    n,
                    in_buf.len()
                );
                outcome.bytes_read = n;

                // if we haven't written all the input, and we haven't gotten
                // any output, then we need to keep going
                if n != 0 && n < in_buf.len() && self.internal_buf_mut().is_empty() {
                    // note: the n != 0 here is because apparently there can be a 10-byte
                    // trailer after LZMA compressed data? and the decoder will _refuse_
                    // to let us write them, so when we have just these 10 bytes left,
                    // it's good to just let the decoder finish up.
                    trace!("didn't write all output AND no output yet, so keep going");
                    return self.decompress(&in_buf[n..], out, has_more_input);
                }

                match has_more_input {
                    HasMoreInput::Yes => {
                        // keep going
                        trace!("more input to come");
                    }
                    HasMoreInput::No => {
                        trace!("no more input to come");
                        match std::mem::take(&mut self.state) {
                            State::Writing(stream) => {
                                trace!("finishing...");
                                self.state = State::Draining(stream.finish().map_err(dec_err)?);
                            }
                            _ => unreachable!(),
                        }
                    }
                }
            }
            State::Draining(_) => {
                // keep going
            }
            State::Transition => unreachable!(),
        }

        self.copy_to_out(out, &mut outcome);
        trace!("decompressor gave us {} bytes", outcome.bytes_written);
        Ok(outcome)
    }
}

fn dec_err(e: impl std::fmt::Display) -> Error {
    Error::Decompression {
        method: Method::Lzma,
        msg: e.to_string(),
    }
}

impl LzmaDec {
    #[inline(always)]
    fn internal_buf_mut(&mut self) -> &mut Vec<u8> {
        match &mut self.state {
            State::Writing(stream) => stream.get_output_mut().unwrap(),
            State::Draining(buf) => buf,
            State::Transition => unreachable!(),
        }
    }

    fn copy_to_out(&mut self, mut out: &mut [u8], outcome: &mut DecompressOutcome) {
        let internal_buf = self.internal_buf_mut();

        while !out.is_empty() && !internal_buf.is_empty() {
            let to_copy = cmp::min(out.len(), internal_buf.len());
            trace!("copying {} bytes from internal buffer", to_copy);
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
