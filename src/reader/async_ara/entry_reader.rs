use crate::{DataDescriptorRecord, Error, FormatError, LocalFileHeaderRecord, Method, StoredEntry};
use ara::{range_reader::RangeReader, ReadAt};
use async_compression::futures::bufread::DeflateDecoder;
use futures::{io::BufReader, AsyncRead, AsyncReadExt, Future};
use nom::Offset;
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

struct EntryReadMetrics {
    data_offset: u64,
    uncompressed_size: u64,
    crc32: u32,
}

enum InnerState<R>
where
    R: ReadAt + Unpin + 'static,
{
    ReadLocalHeader {
        reader: R,
    },
    ReadData {
        hasher: crc32fast::Hasher,
        uncompressed_size: u64,
        header: LocalFileHeaderRecord,
        decoder: Pin<Box<dyn AsyncRead + Unpin>>,
        reader: Arc<R>,
        data_offset: u64,
    },
    ReadDataDescriptor {
        header: LocalFileHeaderRecord,
        metrics: EntryReadMetrics,
        reader: R,
    },
    Validate {
        header: LocalFileHeaderRecord,
        metrics: EntryReadMetrics,
        descriptor: Option<DataDescriptorRecord>,
    },
    Done,
    Transitioning,
}

struct Inner<R>
where
    R: ReadAt + Unpin + 'static,
{
    entry: StoredEntry,
    state: InnerState<R>,
    buffer: Vec<u8>,
}

impl<R> Inner<R>
where
    R: ReadAt + Unpin + 'static,
{
    async fn read(&mut self, read_len: usize) -> io::Result<usize> {
        use InnerState as S;
        let res = loop {
            match self.state {
                S::ReadLocalHeader { ref mut reader } => {
                    let mut header_slice = vec![0u8; 8 * 1024];
                    let mut n: usize = 0;

                    let (remaining, header) = loop {
                        n += reader
                            .read_at(self.entry.header_offset + n as u64, &mut header_slice[n..])
                            .await?;
                        log::debug!("position in header_slice: {}", n);

                        match LocalFileHeaderRecord::parse(&header_slice[..n]) {
                            Ok(res) => {
                                break res;
                            }
                            Err(nom::Err::Incomplete(needed)) => {
                                log::debug!("needed = {:?}", needed);
                                if n >= header_slice.len() {
                                    // TODO: better errors
                                    return Err(io::Error::new(
                                        io::ErrorKind::Other,
                                        "local header is too large",
                                    ));
                                };
                                continue;
                            }
                            Err(_) => {
                                // TODO: better errors
                                return Err(io::Error::new(
                                    io::ErrorKind::Other,
                                    "could not parse local header",
                                ));
                            }
                        }
                    };
                    let local_header_size = header_slice.offset(remaining);
                    let data_offset = self.entry.header_offset + local_header_size as u64;

                    transition!(self.state => (S::ReadLocalHeader { reader }) {
                        let reader = Arc::new(reader);
                        let range_reader = match RangeReader::new(
                            reader.clone(),
                            data_offset..data_offset + self.entry.compressed_size,
                        ) {
                            Ok(r) => r,
                            Err(_) => {
                                return Err(io::Error::new(io::ErrorKind::Other, "out of range error"))
                            }
                        };

                        let decoder: Pin<Box<dyn AsyncRead + Unpin>> =
                            match self.entry.method() {
                                Method::Store => {
                                    // hello
                                    Box::pin(BufReader::new(range_reader))
                                }
                                Method::Deflate => {
                                    // hello
                                    Box::pin(DeflateDecoder::new(BufReader::new(range_reader)))
                                }
                                _other => {
                                    return Err(io::Error::new(
                                        io::ErrorKind::Other,
                                        "unsupported compression method",
                                    ));
                                }
                            };
                        S::ReadData {
                            reader,
                            header,
                            decoder,
                            uncompressed_size: 0,
                            data_offset,
                            hasher: Default::default(),
                        }
                    });
                    // (continue)
                }
                S::ReadData {
                    ref mut hasher,
                    ref mut uncompressed_size,
                    ref mut decoder,
                    ..
                } => {
                    self.buffer.clear();
                    self.buffer.reserve(read_len);
                    unsafe {
                        self.buffer.set_len(read_len);
                    }
                    let n = decoder.read(&mut self.buffer[..read_len]).await?;
                    unsafe {
                        self.buffer.set_len(n);
                    }
                    hasher.update(&self.buffer[..n]);
                    *uncompressed_size += n as u64;

                    if n == 0 {
                        transition!(self.state => (S::ReadData { reader, decoder, header, hasher, uncompressed_size, data_offset }) {
                            let crc32 = hasher.finalize();
                            let metrics = EntryReadMetrics {
                                data_offset,
                                uncompressed_size,
                                crc32,
                            };
                            drop(decoder);
                            let reader = Arc::try_unwrap(reader).map_err(|_| "should be able to get reader back").unwrap();

                            if header.has_data_descriptor() {
                                log::debug!("will read data descriptor (flags = {:x})", header.flags);
                                S::ReadDataDescriptor { reader, header, metrics }
                            } else {
                                log::debug!("no data descriptor to read");
                                S::Validate { metrics, header, descriptor: None }
                            }
                        });
                    }
                    break Ok(n);
                }
                S::ReadDataDescriptor {
                    ref reader,
                    ref metrics,
                    ..
                } => {
                    let descriptor_offset = metrics.data_offset + self.entry.compressed_size;
                    let mut descriptor_slice = vec![0u8; 8 * 1024];
                    let mut n: usize = 0;

                    let (_, descriptor) = loop {
                        n += reader
                            .read_at(descriptor_offset, &mut descriptor_slice[n..])
                            .await?;
                        log::debug!("position in descriptor_slice: {}", n);

                        match DataDescriptorRecord::parse(
                            &descriptor_slice[..n],
                            self.entry.is_zip64,
                        ) {
                            Ok(res) => {
                                break res;
                            }
                            Err(nom::Err::Incomplete(needed)) => {
                                log::debug!("needed = {:?}", needed);
                                if n >= descriptor_slice.len() {
                                    // TODO: better errors
                                    return Err(io::Error::new(
                                        io::ErrorKind::Other,
                                        "data descriptor is too large",
                                    ));
                                };
                                continue;
                            }
                            Err(_) => {
                                // TODO: better errors
                                return Err(io::Error::new(
                                    io::ErrorKind::Other,
                                    "could not parse data descriptor",
                                ));
                            }
                        }
                    };

                    transition!(self.state => (S::ReadDataDescriptor { metrics, header, .. }) {
                        S::Validate { metrics, header, descriptor: Some(descriptor) }
                    });
                }
                S::Validate {
                    ref metrics,
                    ref header,
                    ref descriptor,
                } => {
                    // TODO: dedup with sync EntryReader
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

                    if expected_size != metrics.uncompressed_size {
                        return Err(Error::Format(FormatError::WrongSize {
                            expected: expected_size,
                            actual: metrics.uncompressed_size,
                        })
                        .into());
                    }

                    if expected_crc32 != 0 {
                        log::debug!("expected CRC-32: {:x}", expected_crc32);
                        log::debug!("computed CRC-32: {:x}", metrics.crc32);
                        if expected_crc32 != metrics.crc32 {
                            return Err(Error::Format(FormatError::WrongChecksum {
                                expected: expected_crc32,
                                actual: metrics.crc32,
                            })
                            .into());
                        }
                    }

                    self.state = S::Done;
                }
                S::Done => break Ok(0),
                S::Transitioning => unreachable!(),
            }
        };
        res
    }
}

type PendingFut<R> = Pin<Box<dyn Future<Output = (Inner<R>, io::Result<usize>)> + 'static>>;

enum State<R>
where
    R: ReadAt + Unpin + 'static,
{
    Idle(Inner<R>),
    Pending(PendingFut<R>),
    Transitional,
}

pub struct AsyncEntryReader<R>
where
    R: ReadAt + Unpin + 'static,
{
    state: State<R>,
}

impl<R> AsyncEntryReader<R>
where
    R: ReadAt + Unpin + 'static,
{
    pub fn new(entry: StoredEntry, reader: R) -> Self {
        let inner = Inner {
            state: InnerState::ReadLocalHeader { reader },
            buffer: Default::default(),
            entry,
        };
        Self {
            state: State::Idle(inner),
        }
    }
}

impl<R> AsyncRead for AsyncEntryReader<R>
where
    R: ReadAt + Unpin + 'static,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        let mut state = State::Transitional;
        std::mem::swap(&mut state, &mut self.state);

        let mut fut = match state {
            State::Idle(mut inner) => {
                let read_len = buf.len();

                Box::pin(async move {
                    let res = inner.read(read_len).await;
                    (inner, res)
                })
            }
            State::Pending(fut) => fut,
            State::Transitional => unreachable!(),
        };
        let res = fut.as_mut().poll(cx);

        match res {
            Poll::Ready((inner, res)) => {
                if let Ok(n) = &res {
                    let n = *n;
                    let dst = &mut buf[..n];
                    let src = &inner.buffer[..n];
                    dst.copy_from_slice(src);
                }
                self.state = State::Idle(inner);
                Poll::Ready(res)
            }
            Poll::Pending => {
                self.state = State::Pending(fut);
                Poll::Pending
            }
        }
    }
}
