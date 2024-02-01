use crate::{
    error::*,
    format::*,
    reader::{
        tokio::decoder::{AsyncDecoder, StoreAsyncDecoder},
        RawEntryReader,
    },
    transition_async,
};

use oval::Buffer;
use std::{io, pin::Pin, task};
use tokio::io::AsyncRead;
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

pin_project_lite::pin_project! {
    #[project = StateProj]
    #[derive(Default)]
    enum State {
        ReadLocalHeader {
            buffer: Buffer,
        },
        ReadData {
            hasher: crc32fast::Hasher,
            uncompressed_size: u64,
            header: LocalFileHeaderRecord,
            #[pin]
            decoder: Box<dyn AsyncDecoder<RawEntryReader> + Unpin>,
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
}

pin_project_lite::pin_project! {
    pub struct AsyncEntryReader<R>
    where
        R: AsyncRead,
    {
        #[pin]
        rd: R,
        eof: bool,
        #[pin]
        state: State,
        inner: StoredEntryInner,
        method: Method,
    }
}

impl<R> AsyncRead for AsyncEntryReader<R>
where
    R: AsyncRead,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> task::Poll<io::Result<()>> {
        let mut this = self.as_mut().project();

        use StateProj as S;
        match this.state.as_mut().project() {
            S::ReadLocalHeader { ref mut buffer } => {
                let mut read_buf = tokio::io::ReadBuf::new(buffer.space());
                futures::ready!(this.rd.poll_read(cx, &mut read_buf))?;
                let read_bytes = read_buf.filled().len();
                if read_bytes == 0 {
                    return Err(io::ErrorKind::UnexpectedEof.into()).into();
                }
                buffer.fill(read_bytes);

                let mut input = Partial::new(buffer.data());
                match LocalFileHeaderRecord::parser.parse_next(&mut input) {
                    Ok(header) => {
                        buffer.consume(input.as_bytes().offset_from(&buffer.data()));

                        trace!("local file header: {:#?}", header);
                        transition_async!(this.state => (State::ReadLocalHeader { buffer }) {
                            let decoder = method_to_decoder(*this.method, RawEntryReader::new(buffer, this.inner.compressed_size))?;

                            State::ReadData {
                                hasher: crc32fast::Hasher::new(),
                                uncompressed_size: 0,
                                decoder,
                                header,
                            }
                        });
                        self.poll_read(cx, buf)
                    }
                    Err(ErrMode::Incomplete(_)) => {
                        // try another read - if it returns pending, it'll be propagated
                        self.poll_read(cx, buf)
                    }
                    Err(_e) => Err(Error::Format(FormatError::InvalidLocalHeader).into()).into(),
                }
            }
            S::ReadData {
                ref mut uncompressed_size,
                ref mut decoder,
                ref mut hasher,
                ..
            } => {
                {
                    let buffer = decoder.as_mut().get_mut().get_mut().get_mut();
                    if !*this.eof && buffer.available_data() == 0 {
                        if buffer.available_space() == 0 {
                            buffer.shift();
                        }

                        let mut read_buf = tokio::io::ReadBuf::new(buffer.space());
                        futures::ready!(this.rd.poll_read(cx, &mut read_buf))?;
                        match read_buf.filled().len() {
                            0 => {
                                *this.eof = true;
                            }
                            n => {
                                buffer.fill(n);
                            }
                        }
                    }
                }

                let filled_before = buf.filled().len();
                futures::ready!(decoder.as_mut().poll_read(cx, buf))?;
                let filled_after = buf.filled().len();
                let read_bytes = filled_after - filled_before;

                match read_bytes {
                    0 => {
                        transition_async!(this.state => (State::ReadData { decoder, header, hasher, uncompressed_size, .. }) {
                            let limited_reader = decoder.into_inner();
                            let buffer = limited_reader.into_inner();
                            let metrics = EntryReadMetrics {
                                crc32: hasher.finalize(),
                                uncompressed_size,
                            };
                            if header.has_data_descriptor() {
                                trace!("will read data descriptor (flags = {:x})", header.flags);
                                State::ReadDataDescriptor { metrics, buffer, header }
                            } else {
                                trace!("no data descriptor to read");
                                State::Validate { metrics, header, descriptor: None }
                            }
                        });
                        self.poll_read(cx, buf)
                    }
                    n => {
                        **uncompressed_size += n as u64;
                        let read_slice = &buf.filled()[filled_before..filled_after];
                        hasher.update(read_slice);
                        Ok(()).into()
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
                match DataDescriptorRecord::mk_parser(this.inner.is_zip64).parse_next(&mut input) {
                    Ok(descriptor) => {
                        buffer.consume(input.as_bytes().offset_from(&buffer.data()));
                        trace!("data descriptor = {:#?}", descriptor);
                        transition_async!(this.state => (State::ReadDataDescriptor { metrics, header, .. }) {
                            State::Validate { metrics, header, descriptor: Some(descriptor) }
                        });
                        self.poll_read(cx, buf)
                    }
                    Err(ErrMode::Incomplete(_)) => {
                        let mut read_buf = tokio::io::ReadBuf::new(buffer.space());
                        futures::ready!(this.rd.poll_read(cx, &mut read_buf))?;
                        let read_bytes = read_buf.filled().len();
                        if read_bytes == 0 {
                            return Err(io::ErrorKind::UnexpectedEof.into()).into();
                        }
                        buffer.fill(read_bytes);
                        self.poll_read(cx, buf)
                    }
                    Err(_e) => Err(Error::Format(FormatError::InvalidLocalHeader).into()).into(),
                }
            }
            S::Validate {
                ref metrics,
                ref header,
                ref descriptor,
            } => {
                let expected_crc32 = if this.inner.crc32 != 0 {
                    this.inner.crc32
                } else if let Some(descriptor) = descriptor.as_ref() {
                    descriptor.crc32
                } else {
                    header.crc32
                };

                let expected_size = if this.inner.uncompressed_size != 0 {
                    this.inner.uncompressed_size
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
                    .into())
                    .into();
                }

                if expected_crc32 != 0 && expected_crc32 != metrics.crc32 {
                    return Err(Error::Format(FormatError::WrongChecksum {
                        expected: expected_crc32,
                        actual: metrics.crc32,
                    })
                    .into())
                    .into();
                }

                *this.state.as_mut().get_mut() = State::Done;
                self.poll_read(cx, buf)
            }
            S::Done => Ok(()).into(),
            S::Transitioning => unreachable!(),
        }
    }
}

impl<R> AsyncEntryReader<R>
where
    R: AsyncRead,
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
}

fn method_to_decoder(
    method: Method,
    raw_r: RawEntryReader,
) -> Result<Box<dyn AsyncDecoder<RawEntryReader> + Unpin>, Error> {
    let decoder: Box<dyn AsyncDecoder<RawEntryReader> + Unpin> = match method {
        Method::Store => Box::new(StoreAsyncDecoder::new(raw_r)),
        method => {
            return Err(Error::method_not_supported(method));
        }
    };

    Ok(decoder)
}
