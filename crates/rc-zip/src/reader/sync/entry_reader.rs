//! This part of the API is still being designed - no guarantees are made
//! whatsoever.
use crate::{
    error::*,
    format::*,
    reader::sync::decoder::{Decoder, EOFNormalizer, LimitedReader, StoreDecoder},
    transition,
};

use cfg_if::cfg_if;
use nom::Offset;
use std::io;
use tracing::trace;

#[cfg(feature = "deflate")]
use flate2::read::DeflateDecoder;

struct EntryReadMetrics {
    uncompressed_size: u64,
    crc32: u32,
}

enum State {
    ReadLocalHeader {
        buffer: circular::Buffer,
    },
    ReadData {
        hasher: crc32fast::Hasher,
        uncompressed_size: u64,
        header: LocalFileHeaderRecord,
        decoder: Box<dyn Decoder<LimitedReader>>,
    },
    ReadDataDescriptor {
        metrics: EntryReadMetrics,
        header: LocalFileHeaderRecord,
        buffer: circular::Buffer,
    },
    Validate {
        metrics: EntryReadMetrics,
        header: LocalFileHeaderRecord,
        descriptor: Option<DataDescriptorRecord>,
    },
    Done,
    Transitioning,
}

pub struct EntryReader<R>
where
    R: io::Read,
{
    rd: EOFNormalizer<R>,
    eof: bool,
    state: State,
    inner: StoredEntryInner,
    method: Method,
}

impl<R> io::Read for EntryReader<R>
where
    R: io::Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use State as S;
        match self.state {
            S::ReadLocalHeader { ref mut buffer } => {
                // FIXME: if this returns less than the size of LocalFileHeader, we'll error out
                let read_bytes = self.rd.read(buffer.space())?;
                buffer.fill(read_bytes);

                match LocalFileHeaderRecord::parse(buffer.data()) {
                    Ok((remaining, header)) => {
                        let consumed = buffer.data().offset(remaining);
                        buffer.consume(consumed);

                        trace!("local file header: {:#?}", header);
                        transition!(self.state => (S::ReadLocalHeader { buffer }) {
                            // allow unnecessary mut for some feature combinations
                            #[allow(unused_mut)]
                            let mut limited_reader = LimitedReader::new(buffer, self.inner.compressed_size);
                            let decoder: Box<dyn Decoder<LimitedReader>> = self.get_decoder(limited_reader)?;

                            S::ReadData {
                                hasher: crc32fast::Hasher::new(),
                                uncompressed_size: 0,
                                decoder,
                                header,
                            }
                        });
                        self.read(buf)
                    }
                    Err(_e) => Err(Error::Format(FormatError::InvalidLocalHeader).into()),
                }
            }
            S::ReadData {
                ref mut uncompressed_size,
                ref mut decoder,
                ref mut hasher,
                ..
            } => {
                {
                    let buffer = decoder.get_mut().get_mut();
                    if !self.eof && buffer.available_space() > 0 {
                        match self.rd.read(buffer.space())? {
                            0 => {
                                self.eof = true;
                            }
                            n => {
                                buffer.fill(n);
                            }
                        }
                    }
                }
                match decoder.read(buf) {
                    Ok(0) => {
                        transition!(self.state => (S::ReadData { decoder, header, hasher, uncompressed_size, .. }) {
                            let limited_reader = decoder.into_inner();
                            let buffer = limited_reader.into_inner();
                            let metrics = EntryReadMetrics {
                                crc32: hasher.finalize(),
                                uncompressed_size,
                            };
                            if header.has_data_descriptor() {
                                trace!("will read data descriptor (flags = {:x})", header.flags);
                                S::ReadDataDescriptor { metrics, buffer, header }
                            } else {
                                trace!("no data descriptor to read");
                                S::Validate { metrics, header, descriptor: None }
                            }
                        });
                        self.read(buf)
                    }
                    Ok(n) => {
                        *uncompressed_size += n as u64;
                        hasher.update(&buf[..n]);
                        Ok(n)
                    }
                    Err(e) => match e.kind() {
                        io::ErrorKind::UnexpectedEof => {
                            let buffer = decoder.get_mut().get_mut();
                            if self.eof || buffer.available_space() == 0 {
                                Err(e)
                            } else {
                                self.read(buf)
                            }
                        }
                        _ => Err(e),
                    },
                }
            }
            S::ReadDataDescriptor { ref mut buffer, .. } => {
                trace!(
                    "read data descriptor, avail data = {}, avail space = {}",
                    buffer.available_data(),
                    buffer.available_space()
                );

                match DataDescriptorRecord::parse(buffer.data(), self.inner.is_zip64) {
                    Ok((_remaining, descriptor)) => {
                        trace!("data descriptor = {:#?}", descriptor);
                        transition!(self.state => (S::ReadDataDescriptor { metrics, header, .. }) {
                            S::Validate { metrics, header, descriptor: Some(descriptor) }
                        });
                        self.read(buf)
                    }
                    Err(nom::Err::Incomplete(_)) => {
                        trace!(
                            "incomplete! before shift, data {} / space {}",
                            buffer.available_data(),
                            buffer.available_space()
                        );
                        buffer.shift();
                        trace!(
                            "             after shift, data {} / space {}",
                            buffer.available_data(),
                            buffer.available_space()
                        );
                        let n = self.rd.read(buffer.space())?;
                        if n == 0 {
                            return Err(io::ErrorKind::UnexpectedEof.into());
                        }
                        buffer.fill(n);
                        trace!("filled {}", n);

                        self.read(buf)
                    }
                    Err(_e) => Err(Error::Format(FormatError::InvalidLocalHeader).into()),
                }
            }
            S::Validate {
                ref metrics,
                ref header,
                ref descriptor,
            } => {
                let expected_crc32 = if self.inner.crc32 != 0 {
                    self.inner.crc32
                } else if let Some(descriptor) = descriptor.as_ref() {
                    descriptor.crc32
                } else {
                    header.crc32
                };

                let expected_size = if self.inner.uncompressed_size != 0 {
                    self.inner.uncompressed_size
                } else if let Some(descriptor) = descriptor.as_ref() {
                    descriptor.uncompressed_size
                } else {
                    header.uncompressed_size as u64
                };

                if expected_size != metrics.uncompressed_size {
                    return Err(Error::Format(FormatError::WrongSize {
                        expected: expected_size,
                        actual: metrics.uncompressed_size,
                    })
                    .into());
                }

                if expected_crc32 != 0 && expected_crc32 != metrics.crc32 {
                    return Err(Error::Format(FormatError::WrongChecksum {
                        expected: expected_crc32,
                        actual: metrics.crc32,
                    })
                    .into());
                }

                self.state = S::Done;
                self.read(buf)
            }
            S::Done => Ok(0),
            S::Transitioning => unreachable!(),
        }
    }
}

impl<R> EntryReader<R>
where
    R: io::Read,
{
    const DEFAULT_BUFFER_SIZE: usize = 8 * 1024;

    pub fn new<F>(entry: &StoredEntry, get_reader: F) -> Self
    where
        F: Fn(u64) -> R,
    {
        Self {
            rd: EOFNormalizer::new(get_reader(entry.header_offset)),
            eof: false,
            state: State::ReadLocalHeader {
                buffer: circular::Buffer::with_capacity(Self::DEFAULT_BUFFER_SIZE),
            },
            method: entry.method(),
            inner: entry.inner,
        }
    }

    fn get_decoder(
        &self,
        #[allow(unused_mut)] mut limited_reader: LimitedReader,
    ) -> std::io::Result<Box<dyn Decoder<LimitedReader>>> {
        let decoder: Box<dyn Decoder<LimitedReader>> = match self.method {
            Method::Store => Box::new(StoreDecoder::new(limited_reader)),
            Method::Deflate => {
                cfg_if! {
                    if #[cfg(feature = "deflate")] {
                        Box::new(DeflateDecoder::new(limited_reader))
                    } else {
                        return Err(
                            Error::Unsupported(UnsupportedError::CompressionMethodNotEnabled(
                                Method::Deflate,
                            ))
                            .into(),
                        );
                    }
                }
            }
            Method::Lzma => {
                cfg_if! {
                    if #[cfg(feature = "lzma")] {
                        // TODO: use a parser combinator library for this probably?

                        // read LZMA properties header first.
                        use byteorder::{LittleEndian, ReadBytesExt};
                        let major: u8 = limited_reader.read_u8()?;
                        let minor: u8 = limited_reader.read_u8()?;
                        if (major, minor) != (2, 0) {
                            return Err(
                                Error::Unsupported(UnsupportedError::LzmaVersionUnsupported {
                                    minor,
                                    major,
                                })
                                .into(),
                            );
                        }

                        let props_size: u16 = limited_reader.read_u16::<LittleEndian>()?;

                        const LZMA_2_0_PROPS_SIZE: u16 = 5;
                        if props_size != LZMA_2_0_PROPS_SIZE {
                            return Err(Error::Unsupported(
                                UnsupportedError::LzmaPropertiesHeaderTooShort {
                                    expected: 5,
                                    actual: props_size,
                                },
                            )
                            .into());
                        }
                        let bits_byte: u8 = limited_reader.read_u8()?;

                        #[derive(Debug, Clone, Copy)]
                        struct LzmaProperties {
                            literal_context_bits: u8,
                            literal_pos_state_bits: u8,
                            pos_state_bits: u8,
                        }

                        // from `lzma-specification.txt`
                        fn decode_properties(mut d: u8) -> Result<LzmaProperties, FormatError> {
                            if d >= (9 * 5 * 5) {
                                return Err(FormatError::LzmaPropertiesLargerThanMax);
                            }

                            let lc = d % 9;
                            d /= 9;
                            let pb = d / 5;
                            let lp = d % 5;

                            Ok(LzmaProperties {
                                literal_context_bits: lc,
                                literal_pos_state_bits: lp,
                                pos_state_bits: pb,
                            })
                        }

                        let props = decode_properties(bits_byte);
                        trace!("LZMA properties: {:#?}", props);

                        const LZMA_DIC_MIN: u32 = 1 << 12;
                        let dict_size_read = limited_reader.read_u32::<LittleEndian>()?;
                        trace!("LZMA dictionary size (raw): {}", dict_size_read);
                        let dict_size: u32 =
                            std::cmp::max(LZMA_DIC_MIN, dict_size_read);
                        trace!("LZMA dictionary size: {}", dict_size);

                        // let mut opts = xz2::stream::LzmaOptions::new_preset(0)?;
                        // opts.dict_size(dict_size);
                        // opts.position_bits(props.pos_state_bits as _);
                        // opts.literal_position_bits(props.literal_pos_state_bits as _);
                        // opts.literal_context_bits(props.literal_context_bits as _);

                        // let mut filters = xz2::stream::Filters::new();
                        // filters.lzma2(&opts);

                        // uncompressed size is stored as a little-endian 64-bit integer
                        let uncompressed_size: u64 = limited_reader.read_u64::<LittleEndian>()?;
                        trace!("LZMA uncompressed size: {}", uncompressed_size);

                        let stream = xz2::stream::Stream::new_lzma_decoder(128 * 1024 * 1024)?;

                        Box::new(xz2::read::XzDecoder::new_stream(limited_reader, stream))
                    } else {
                        return Err(
                            Error::Unsupported(UnsupportedError::CompressionMethodNotEnabled(
                                Method::Lzma,
                            ))
                            .into(),
                        );
                    }
                }
            }
            method => {
                return Err(
                    Error::Unsupported(UnsupportedError::UnsupportedCompressionMethod(method))
                        .into(),
                )
            }
        };

        Ok(decoder)
    }
}
