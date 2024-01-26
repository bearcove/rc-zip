use std::io::{BufReader, Read};
use tracing::trace;

use crate::{
    reader::sync::{Decoder, LimitedReader},
    Error, UnsupportedError,
};

struct LzmaDecoderAdapter<R> {
    input: BufReader<R>,
    raw: lzma_rs::decompress::raw::LzmaDecoder,
    buf: Vec<u8>,
    total_write_count: u64,
}

impl<R> Read for LzmaDecoderAdapter<R>
where
    R: Read,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if let Err(e) = self.raw.decompress(&mut self.input, &mut self.buf) {
            trace!("LzmaDecoderAdapter::read, got error {e:?}");
            return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
        }

        let write_count = std::cmp::min(buf.len(), self.buf.len());
        {
            let src_slice = &self.buf[..write_count];
            let dst_slice = &mut buf[..write_count];
            dst_slice.copy_from_slice(src_slice);
        }

        // TODO: use a ring buffer instead
        self.buf = self.buf.split_off(write_count);

        self.total_write_count += write_count as u64;
        trace!(
            "LzmaDecoderAdapter::read, returning {write_count} bytes, total_write_count = {}",
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
        self.input.into_inner()
    }

    fn get_mut(&mut self) -> &mut R {
        self.input.get_mut()
    }
}

pub(crate) fn mk_decoder(
    mut r: LimitedReader,
    uncompressed_size: u64,
    flags: u16,
) -> std::io::Result<Box<dyn Decoder<LimitedReader>>> {
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
        allow_incomplete: true,
        memlimit: Some(memlimit),
    };
    let mut limited_reader = BufReader::new(r);
    let params = lzma_rs::decompress::raw::LzmaParams::read_header(&mut limited_reader, &opts)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    trace!(?params, "Read lzma params");

    // general-purpose bit flag 1 indicates that the stream has an EOS marker
    let has_eos = flags & 0b01 != 0;
    trace!(?has_eos, "EOS marker?, flags = {flags:x?}");

    let dec = lzma_rs::decompress::raw::LzmaDecoder::new(params, Some(memlimit))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(Box::new(LzmaDecoderAdapter {
        input: limited_reader,
        raw: dec,
        buf: Vec::new(),
        total_write_count: 0,
    }))
}
