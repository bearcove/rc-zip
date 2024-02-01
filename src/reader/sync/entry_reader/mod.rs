//! This part of the API is still being designed - no guarantees are made
//! whatsoever.
use crate::{
    error::*,
    format::*,
    reader::{
        sync::decoder::{Decoder, StoreDecoder},
        RawEntryReader,
    },
    transition,
};

#[cfg(feature = "lzma")]
mod lzma_dec;

#[cfg(feature = "deflate")]
mod deflate_dec;

#[cfg(feature = "deflate64")]
mod deflate64_dec;

#[cfg(feature = "bzip2")]
mod bzip2_dec;

#[cfg(feature = "zstd")]
mod zstd_dec;

use cfg_if::cfg_if;
use oval::Buffer;
use std::io;
use tracing::trace;
use winnow::{
    error::ErrMode,
    stream::{AsBytes, Offset},
    Parser, Partial,
};

struct EntryReadMetrics {
    uncompressed_size: u64,
    crc32: u32,
}

#[derive(Default)]
enum State {
    ReadLocalHeader {
        buffer: Buffer,
    },
    ReadData {
        hasher: crc32fast::Hasher,
        uncompressed_size: u64,
        header: LocalFileHeaderRecord,
        decoder: Box<dyn Decoder<RawEntryReader>>,
    },
    ReadDataDescriptor {
        metrics: EntryReadMetrics,
        header: LocalFileHeaderRecord,
        buffer: Buffer,
    },
    Validate {
        metrics: EntryReadMetrics,
        header: LocalFileHeaderRecord,
        descriptor: Option<DataDescriptorRecord>,
    },
    Done,
    #[default]
    Transitioning,
}

pub struct EntryReader<R>
where
    R: io::Read,
{
    rd: R,
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
                let read_bytes = self.rd.read(buffer.space())?;
                if read_bytes == 0 {
                    // we should have read the local header by now
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
                buffer.fill(read_bytes);

                let mut input = Partial::new(buffer.data());
                match LocalFileHeaderRecord::parser.parse_next(&mut input) {
                    Ok(header) => {
                        buffer.consume(input.as_bytes().offset_from(&buffer.data()));

                        trace!("local file header: {:#?}", header);
                        transition!(self.state => (S::ReadLocalHeader { buffer }) {
                            let decoder = self.get_decoder(RawEntryReader::new(buffer, self.inner.compressed_size))?;

                            S::ReadData {
                                hasher: crc32fast::Hasher::new(),
                                uncompressed_size: 0,
                                decoder,
                                header,
                            }
                        });
                        self.read(buf)
                    }
                    Err(ErrMode::Incomplete(_)) => self.read(buf),
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
                    if !self.eof && buffer.available_data() == 0 {
                        if buffer.available_space() == 0 {
                            buffer.shift();
                        }

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
                match decoder.read(buf)? {
                    0 => {
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
                    n => {
                        *uncompressed_size += n as u64;
                        hasher.update(&buf[..n]);
                        Ok(n)
                    }
                }
            }
            S::ReadDataDescriptor { ref mut buffer, .. } => {
                trace!(
                    "read data descriptor, avail data = {}, avail space = {}",
                    buffer.available_data(),
                    buffer.available_space()
                );

                let mut input = Partial::new(buffer.data());
                match DataDescriptorRecord::mk_parser(self.inner.is_zip64).parse_next(&mut input) {
                    Ok(descriptor) => {
                        buffer.consume(input.as_bytes().offset_from(&buffer.data()));
                        trace!("data descriptor = {:#?}", descriptor);
                        transition!(self.state => (S::ReadDataDescriptor { metrics, header, .. }) {
                            S::Validate { metrics, header, descriptor: Some(descriptor) }
                        });
                        self.read(buf)
                    }
                    Err(ErrMode::Incomplete(_)) => {
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
    const DEFAULT_BUFFER_SIZE: usize = 256 * 1024;

    pub fn new<F>(entry: &StoredEntry, get_reader: F) -> Self
    where
        F: Fn(u64) -> R,
    {
        Self {
            rd: get_reader(entry.header_offset),
            eof: false,
            state: State::ReadLocalHeader {
                buffer: Buffer::with_capacity(Self::DEFAULT_BUFFER_SIZE),
            },
            method: entry.method(),
            inner: entry.inner,
        }
    }

    fn get_decoder(
        &self,
        raw_r: RawEntryReader,
    ) -> Result<Box<dyn Decoder<RawEntryReader>>, Error> {
        let decoder: Box<dyn Decoder<RawEntryReader>> = match self.method {
            Method::Store => Box::new(StoreDecoder::new(raw_r)),
            Method::Deflate => {
                cfg_if! {
                    if #[cfg(feature = "deflate")] {
                        Box::new(deflate_dec::mk_decoder(raw_r))
                    } else {
                        return Err(Error::method_not_enabled(self.method));
                    }
                }
            }
            Method::Deflate64 => {
                cfg_if! {
                    if #[cfg(feature = "deflate64")] {
                        Box::new(deflate64_dec::mk_decoder(raw_r))
                    } else {
                        return Err(Error::method_not_enabled(self.method));
                    }
                }
            }
            Method::Lzma => {
                cfg_if! {
                    if #[cfg(feature = "lzma")] {
                        Box::new(lzma_dec::mk_decoder(raw_r,self.inner.uncompressed_size)?)
                    } else {
                        return Err(Error::method_not_enabled(self.method));
                    }
                }
            }
            Method::Bzip2 => {
                cfg_if! {
                    if #[cfg(feature = "bzip2")] {
                        Box::new(bzip2_dec::mk_decoder(raw_r))
                    } else {
                        return Err(Error::method_not_enabled(self.method));
                    }
                }
            }
            Method::Zstd => {
                cfg_if! {
                    if #[cfg(feature = "zstd")] {
                        Box::new(zstd_dec::mk_decoder(raw_r)?)
                    } else {
                        return Err(Error::method_not_enabled(self.method));
                    }
                }
            }
            method => {
                return Err(Error::method_not_supported(method));
            }
        };

        Ok(decoder)
    }
}
