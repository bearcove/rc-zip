use lzma_rs::decompress::Stream;
use std::io::{Read, Write};
use tracing::trace;

use crate::{
    reader::sync::{Decoder, LimitedReader},
    Error, UnsupportedError,
};

enum LzmaDecoderState {
    Writing(Box<Stream<Vec<u8>>>),
    Draining(Vec<u8>),
    Transition,
}
struct LzmaDecoderAdapter<R> {
    input: R,
    total_write_count: u64,
    state: LzmaDecoderState,
}

impl<R> Read for LzmaDecoderAdapter<R>
where
    R: Read,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut state = LzmaDecoderState::Transition;
        std::mem::swap(&mut state, &mut self.state);

        match state {
            LzmaDecoderState::Writing(mut stream) => {
                // FIXME: all this is terribly wasteful, I'm just trying to see if it
                // will decompress
                let mut read_buf = vec![0u8; 8192];
                let bytes_read = self.input.read(&mut read_buf)?;
                if bytes_read == 0 {
                    // we're EOF: finish and move on to draining
                    self.state = LzmaDecoderState::Draining(stream.finish()?);
                    // and recurse
                    return self.read(buf);
                }

                trace!(
                    "Writing {} bytes to lzma_rs::decompress::Stream",
                    bytes_read
                );
                if let Err(e) = stream.write_all(&read_buf[..bytes_read]) {
                    if e.kind() == std::io::ErrorKind::WriteZero {
                        // that's expected actually! from the lzma-rs tests:
                        //
                        // A WriteZero error may occur if decompression is finished but there
                        // are remaining `compressed` bytes to write.
                        // This is the case when the unpacked size is encoded as unknown but
                        // provided when decoding. I.e. the 5 or 6 byte end-of-stream marker
                        // is not read.
                        trace!("WriteZero error, flushing lzma_rs::decompress::Stream");

                        // finish and move on to draining
                        self.state = LzmaDecoderState::Draining(stream.finish()?);
                        // and recurse
                        return self.read(buf);
                    } else {
                        trace!("Error writing to lzma_rs::decompress::Stream: {:?}", e);
                        return Err(e);
                    }
                }

                self.state = LzmaDecoderState::Writing(stream);
            }
            LzmaDecoderState::Draining(vec) => {
                // nothing more to decode, we just need to empty our
                // internal buffer
                self.state = LzmaDecoderState::Draining(vec);
            }
            LzmaDecoderState::Transition => {
                unreachable!()
            }
        };

        let write_buf = match &mut self.state {
            LzmaDecoderState::Writing(stream) => stream.get_output_mut().unwrap(),
            LzmaDecoderState::Draining(vec) => vec,
            LzmaDecoderState::Transition => unreachable!(),
        };
        trace!("write_buf.len() = {}", write_buf.len());
        let write_count = std::cmp::min(buf.len(), write_buf.len());
        {
            let src_slice = &write_buf[..write_count];
            let dst_slice = &mut buf[..write_count];
            dst_slice.copy_from_slice(src_slice);
        }

        // TODO: use a ring buffer instead
        *write_buf = write_buf.split_off(write_count);

        self.total_write_count += write_count as u64;
        trace!(
            "lzma_rs::decompress::Stream has returned {write_count} bytes, total = {}",
            self.total_write_count
        );

        Ok(write_count)
    }
}

impl<R> Decoder<R> for LzmaDecoderAdapter<R>
where
    R: Read,
{
    fn into_inner(self: Box<Self>) -> R {
        self.input
    }

    fn get_mut(&mut self) -> &mut R {
        &mut self.input
    }
}

pub(crate) fn mk_decoder(
    mut r: LimitedReader,
    uncompressed_size: u64,
) -> std::io::Result<impl Decoder<LimitedReader>> {
    use byteorder::{LittleEndian, ReadBytesExt};

    // see `appnote.txt` section 5.8

    // major & minor version are each 1 byte
    let major = r.read_u8()?;
    let minor = r.read_u8()?;

    // properties size is a 2-byte little-endian integer
    let properties_size = r.read_u16::<LittleEndian>()?;

    if (major, minor) != (2, 0) {
        return Err(
            Error::Unsupported(UnsupportedError::LzmaVersionUnsupported { minor, major }).into(),
        );
    }

    const LZMA_PROPERTIES_SIZE: u16 = 5;
    if properties_size != LZMA_PROPERTIES_SIZE {
        return Err(
            Error::Unsupported(UnsupportedError::LzmaPropertiesHeaderWrongSize {
                expected: 5,
                actual: properties_size,
            })
            .into(),
        );
    }

    let memlimit = 128 * 1024 * 1024;
    let opts = lzma_rs::decompress::Options {
        unpacked_size: lzma_rs::decompress::UnpackedSize::UseProvided(Some(uncompressed_size)),
        allow_incomplete: false,
        memlimit: Some(memlimit),
    };

    let stream = Stream::new_with_options(&opts, vec![]);
    Ok(LzmaDecoderAdapter {
        input: r,
        total_write_count: 0,
        state: LzmaDecoderState::Writing(Box::new(stream)),
    })
}
