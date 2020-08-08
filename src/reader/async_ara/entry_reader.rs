use super::AsyncDecoder;
use crate::{LocalFileHeaderRecord, Method, StoredEntry};
use ara::{range_reader::RangeReader, ReadAt};
use async_compression::futures::bufread::DeflateDecoder;
use futures::{io::BufReader, AsyncRead, AsyncReadExt, Future};
use nom::Offset;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

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
        decoder: Pin<Box<dyn AsyncDecoder<BufReader<RangeReader<R>>>>>,
    },
    ReadDataDescriptor {
        header: LocalFileHeaderRecord,
        reader: R,
    },
    Validate {
        header: LocalFileHeaderRecord,
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
                        log::debug!("n is now: {}", n);

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
                            Err(e) => {
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
                        let range_reader = match RangeReader::new(
                            reader,
                            data_offset..data_offset + self.entry.compressed_size,
                        ) {
                            Ok(r) => r,
                            Err(e) => {
                                return Err(io::Error::new(io::ErrorKind::Other, "out of range error"))
                            }
                        };

                        let decoder: Pin<Box<dyn AsyncDecoder<BufReader<RangeReader<R>>>>> =
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
                            header,
                            decoder,
                            uncompressed_size: 0,
                            hasher: Default::default(),
                        }
                    });
                    // (continue)
                }
                S::ReadData {
                    ref mut hasher,
                    ref mut uncompressed_size,
                    ref header,
                    ref mut decoder,
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

                    if n == 0 {
                        self.state = S::Done;
                    }
                    break Ok(n);
                }
                S::ReadDataDescriptor {
                    ref reader,
                    ref header,
                } => todo!(),
                S::Validate { ref header } => todo!(),
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
