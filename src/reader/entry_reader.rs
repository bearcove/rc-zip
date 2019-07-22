//! This part of the API is still under designs - no guarantees are made
//! whatsoever.
use crate::{error::*, format::*};

use libflate::non_blocking::deflate;
use log::*;
use nom::Offset;
use std::io::Read;

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
        decoder: Box<Decoder<circular::Buffer>>,
        read_bytes: u64,
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
    R: Read,
{
    entry: &'a StoredEntry,
    rd: R,
    state: EntryReaderState,
}

impl<'a, R> Read for EntryReader<'a, R>
where
    R: Read,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use EntryReaderState as S;
        match self.state {
            S::ReadLocalHeader { ref mut buffer } => {
                if buffer.available_data() < 4 {
                    let read_bytes = self.rd.read(buffer.space())?;
                    buffer.fill(read_bytes);
                }

                match LocalFileHeaderRecord::parse(buffer.data()) {
                    Ok((remaining, header)) => {
                        let consumed = buffer.data().offset(remaining);
                        drop(remaining);
                        buffer.consume(consumed);
                        drop(buffer);

                        debug!("local file header: {:#?}", header);
                        transition!(self.state => (S::ReadLocalHeader { buffer }) {
                            let read_bytes = std::cmp::min(buffer.available_data() as u64, self.entry.compressed_size);
                            let decoder: Box<Decoder<circular::Buffer>> = match self.entry.method() {
                                Method::Store => Box::new(buffer),
                                Method::Deflate => Box::new(deflate::Decoder::new(buffer)),
                                method => return Err(Error::Unsupported(UnsupportedError::UnsupportedCompressionMethod(method)).into()),
                            };

                            S::ReadData {
                                hasher: crc32fast::Hasher::new(),
                                uncompressed_size: 0,
                                decoder,
                                header,
                                read_bytes,
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
                ref mut read_bytes,
                ref mut hasher,
                ..
            } => {
                let remaining = self.entry.compressed_size - *read_bytes;
                if remaining > 0 {
                    let buffer = decoder.as_inner_mut();
                    let avail_space = buffer.available_space() as u64;
                    if avail_space > 0 {
                        let space = if remaining < avail_space {
                            &mut buffer.space()[..remaining as usize]
                        } else {
                            buffer.space()
                        };

                        let n = self.rd.read(space)?;
                        buffer.fill(n);
                    }
                }
                match decoder.read(buf) {
                    Ok(0) => {
                        transition!(self.state => (S::ReadData { decoder, header, hasher, uncompressed_size, .. }) {
                            let buffer = decoder.into_inner();
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
                    let read_bytes = dbg!(self.rd.read(buffer.space()))?;
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
    R: Read,
{
    pub fn new<F>(entry: &'a StoredEntry, get_reader: F) -> Self
    where
        F: Fn(u64) -> R,
    {
        debug!("entry: {:#?}", entry);
        Self {
            entry,
            rd: get_reader(entry.header_offset),
            state: EntryReaderState::ReadLocalHeader {
                buffer: circular::Buffer::with_capacity(128 * 1024),
            },
        }
    }
}

trait Decoder<R>: Read
where
    R: Read,
{
    fn into_inner(self: Box<Self>) -> R;
    fn as_inner_mut<'a>(&'a mut self) -> &'a mut R;
}

impl<R> Decoder<R> for deflate::Decoder<R>
where
    R: Read,
{
    fn into_inner(self: Box<Self>) -> R {
        deflate::Decoder::into_inner(*self)
    }

    fn as_inner_mut<'a>(&'a mut self) -> &'a mut R {
        deflate::Decoder::as_inner_mut(self)
    }
}

impl Decoder<circular::Buffer> for circular::Buffer {
    fn into_inner(self: Box<Self>) -> circular::Buffer {
        *self
    }

    fn as_inner_mut<'a>(&'a mut self) -> &'a mut circular::Buffer {
        self
    }
}
