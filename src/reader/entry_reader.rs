//! This part of the API is still being designed - no guarantees are made
//! whatsoever.
use crate::{
    error::*,
    format::*,
    reader::decoder::{Decoder, EOFNormalizer, LimitedReader, StoreDecoder},
};

use libflate::non_blocking::deflate;
use log::*;
use nom::Offset;
use std::io;

struct EntryReadMetrics {
    uncompressed_size: u64,
    crc32: u32,
}

enum EntryReaderState {
    ReadLocalHeader {
        buffer: circular::Buffer,
    },
    ReadData {
        hasher: crc32fast::Hasher,
        uncompressed_size: u64,
        header: LocalFileHeaderRecord,
        decoder: Box<Decoder<LimitedReader<circular::Buffer>>>,
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

pub enum EntryReaderResult {
    Continue,
    Done,
}

pub struct EntryReader<'a, R>
where
    R: io::Read,
{
    entry: &'a StoredEntry,
    rd: EOFNormalizer<R>,
    eof: bool,
    state: EntryReaderState,
}

impl<'a, R> io::Read for EntryReader<'a, R>
where
    R: io::Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use EntryReaderState as S;
        match self.state {
            S::ReadLocalHeader { ref mut buffer } => {
                // FIXME: that's wrong, use Needed() instead
                let read_bytes = self.rd.read(buffer.space())?;
                buffer.fill(read_bytes);

                match LocalFileHeaderRecord::parse(buffer.data()) {
                    Ok((remaining, header)) => {
                        let consumed = buffer.data().offset(remaining);
                        drop(remaining);
                        buffer.consume(consumed);
                        drop(buffer);

                        debug!("local file header: {:#?}", header);
                        transition!(self.state => (S::ReadLocalHeader { buffer }) {
                            let limited_reader = LimitedReader::new(buffer, self.entry.compressed_size);
                            let decoder: Box<Decoder<LimitedReader<circular::Buffer>>> = match self.entry.method() {
                                Method::Store => Box::new(StoreDecoder::new(limited_reader)),
                                Method::Deflate => Box::new(deflate::Decoder::new(limited_reader)),
                                method => return Err(Error::Unsupported(UnsupportedError::UnsupportedCompressionMethod(method)).into()),
                            };

                            S::ReadData {
                                hasher: crc32fast::Hasher::new(),
                                uncompressed_size: 0,
                                decoder,
                                header,
                            }
                        });
                        self.read(buf)
                    }
                    Err(_e) => return Err(Error::Format(FormatError::InvalidLocalHeader).into()),
                }
            }
            S::ReadData {
                ref mut uncompressed_size,
                ref mut decoder,
                ref mut hasher,
                ..
            } => {
                if !self.eof {
                    let buffer = decoder.as_inner_mut().as_inner_mut();
                    if buffer.available_space() < buffer.available_data() {
                        buffer.shift();
                        if buffer.available_space() > 0 {
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
                                debug!("will read data descriptor (flags = {:x})", header.flags);
                                S::ReadDataDescriptor { metrics, buffer, header }
                            } else {
                                debug!("no data descriptor to read");
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
                    r => r,
                }
            }
            S::ReadDataDescriptor { ref mut buffer, .. } => {
                debug!(
                    "read data descriptor, avail data = {}, avail space = {}",
                    buffer.available_data(),
                    buffer.available_space()
                );

                // FIXME: should this be a loop? should it error out
                // on read_bytes == 0 ?
                if buffer.available_data() < 4 {
                    let read_bytes = self.rd.read(buffer.space())?;
                    buffer.fill(read_bytes);
                }

                match DataDescriptorRecord::parse(buffer.data(), self.entry.is_zip64) {
                    Ok((_remaining, descriptor)) => {
                        debug!("data descriptor = {:#?}", descriptor);
                        transition!(self.state => (S::ReadDataDescriptor { metrics, header, .. }) {
                            S::Validate { metrics, header, descriptor: Some(descriptor) }
                        });
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
                let expected_crc32 = if self.entry.crc32 != 0 {
                    self.entry.crc32
                } else {
                    if let Some(descriptor) = descriptor.as_ref() {
                        descriptor.crc32
                    } else {
                        header.crc32
                    }
                };

                let expected_size = if self.entry.uncompressed_size != 0 {
                    self.entry.uncompressed_size
                } else {
                    if let Some(descriptor) = descriptor.as_ref() {
                        descriptor.uncompressed_size
                    } else {
                        header.uncompressed_size as u64
                    }
                };

                if expected_crc32 != 0 {
                    debug!("expected CRC-32: {:x}", expected_crc32);
                    debug!("computed CRC-32: {:x}", metrics.crc32);
                    if expected_crc32 != metrics.crc32 {
                        return Err(Error::Format(FormatError::WrongChecksum {
                            expected: expected_crc32,
                            actual: metrics.crc32,
                        })
                        .into());
                    }
                }

                if expected_size != metrics.uncompressed_size {
                    return Err(Error::Format(FormatError::WrongSize {
                        expected: expected_size,
                        actual: metrics.uncompressed_size,
                    })
                    .into());
                }

                self.state = S::Done;
                self.read(buf)
            }
            S::Done => Ok(0),
            _ => unimplemented!(),
        }
    }
}

impl<'a, R> EntryReader<'a, R>
where
    R: io::Read,
{
    pub fn new<F>(entry: &'a StoredEntry, get_reader: F) -> Self
    where
        F: Fn(u64) -> R,
    {
        debug!("entry: {:#?}", entry);
        Self {
            entry,
            rd: EOFNormalizer::new(get_reader(entry.header_offset)),
            eof: false,
            state: EntryReaderState::ReadLocalHeader {
                buffer: circular::Buffer::with_capacity(128 * 1024),
            },
        }
    }
}
