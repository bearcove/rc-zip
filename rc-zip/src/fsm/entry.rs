// FIXME: remove
#![allow(unused)]

use oval::Buffer;
use tracing::trace;
use winnow::{
    error::ErrMode,
    stream::{AsBytes, Offset},
    Parser, Partial,
};

mod store_dec;

#[cfg(feature = "deflate")]
mod deflate_dec;

use crate::{
    error::{Error, FormatError, UnsupportedError},
    parse::{DataDescriptorRecord, LocalFileHeaderRecord, Method, StoredEntryInner},
};

use super::FsmResult;

struct EntryReadMetrics {
    uncompressed_size: u64,
    crc32: u32,
}

#[derive(Default)]
enum State {
    ReadLocalHeader,

    ReadData {
        /// The local file header for this entry
        header: LocalFileHeaderRecord,

        /// Amount of data we have decompressed so far
        uncompressed_size: u64,

        /// CRC32 hash of the decompressed data
        hasher: crc32fast::Hasher,

        /// The decompression method we're using
        decompressor: AnyDecompressor,
    },

    ReadDataDescriptor {
        /// The local file header for this entry
        header: LocalFileHeaderRecord,

        /// Size we've decompressed + crc32 hash we've computed
        metrics: EntryReadMetrics,
    },

    Validate {
        /// The local file header for this entry
        header: LocalFileHeaderRecord,

        /// Size we've decompressed + crc32 hash we've computed
        metrics: EntryReadMetrics,

        /// The data descriptor for this entry, if any
        descriptor: Option<DataDescriptorRecord>,
    },

    #[default]
    Transition,
}

/// A state machine that can parse a zip entry
pub struct EntryFsm {
    state: State,
    entry: StoredEntryInner,
    method: Method,
    buffer: Buffer,
    eof: bool,
}

impl EntryFsm {
    /// Create a new state machine for decompressing a zip entry
    pub fn new(method: Method, entry: StoredEntryInner) -> Self {
        Self {
            state: State::ReadLocalHeader,
            entry,
            method,
            buffer: Buffer::with_capacity(256 * 1024),
            eof: false,
        }
    }

    /// If this returns true, the caller should read data from into
    /// [Self::space] — without forgetting to call [Self::fill] with the number
    /// of bytes written.
    pub fn wants_read(&self) -> bool {
        match self.state {
            State::ReadLocalHeader => true,
            State::ReadData { .. } => {
                // we want to read if we have space
                self.buffer.available_space() > 0
            }
            State::ReadDataDescriptor { .. } => true,
            State::Validate { .. } => false,
            State::Transition => false,
        }
    }

    pub fn process(
        mut self,
        out: &mut [u8],
    ) -> Result<FsmResult<(Self, DecompressOutcome), ()>, Error> {
        use State as S;
        match &mut self.state {
            S::ReadLocalHeader => {
                let mut input = Partial::new(self.buffer.data());
                match LocalFileHeaderRecord::parser.parse_next(&mut input) {
                    Ok(header) => {
                        self.state = S::ReadData {
                            header,
                            uncompressed_size: 0,
                            hasher: crc32fast::Hasher::new(),
                            decompressor: AnyDecompressor::new(self.method)?,
                        };
                        self.process(out)
                    }
                    Err(ErrMode::Incomplete(_)) => {
                        Ok(FsmResult::Continue((self, Default::default())))
                    }
                    Err(_e) => Err(Error::Format(FormatError::InvalidLocalHeader)),
                }
            }
            S::ReadData {
                header,
                uncompressed_size,
                hasher,
                decompressor,
            } => {
                let in_buf = self.buffer.data();
                let is_flushing = in_buf.is_empty();
                let outcome = decompressor.decompress(in_buf, out)?;
                self.buffer.consume(outcome.bytes_read);

                if outcome.bytes_written == 0 && self.eof {
                    // we're done, let's read the data descriptor (if there's one)
                    transition!(self.state => (S::ReadData { header, uncompressed_size, hasher, decompressor }) {
                        S::ReadDataDescriptor {
                            header,
                            metrics: EntryReadMetrics {
                                uncompressed_size,
                                crc32: hasher.finalize(),
                            },
                        }
                    });
                    return self.process(out);
                }
                Ok(FsmResult::Continue((self, outcome)))
            }
            S::ReadDataDescriptor { header, metrics } => {
                let mut input = Partial::new(self.buffer.data());
                match DataDescriptorRecord::mk_parser(self.entry.is_zip64).parse_next(&mut input) {
                    Ok(descriptor) => {
                        self.buffer
                            .consume(input.as_bytes().offset_from(&self.buffer.data()));
                        trace!("data descriptor = {:#?}", descriptor);
                        transition!(self.state => (S::ReadDataDescriptor { metrics, header, .. }) {
                            S::Validate { metrics, header, descriptor: Some(descriptor) }
                        });
                        self.process(out)
                    }
                    Err(ErrMode::Incomplete(_)) => {
                        Ok(FsmResult::Continue((self, Default::default())))
                    }
                    Err(_e) => Err(Error::Format(FormatError::InvalidDataDescriptor).into()),
                }
            }
            S::Validate {
                header,
                metrics,
                descriptor,
            } => {
                let expected_crc32 = if self.entry.crc32 != 0 {
                    self.entry.crc32
                } else if let Some(descriptor) = descriptor.as_ref() {
                    descriptor.crc32
                } else {
                    header.crc32
                };

                let expected_size = if self.entry.uncompressed_size != 0 {
                    self.entry.uncompressed_size
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

                Ok(FsmResult::Done(()))
            }
            S::Transition => {
                unreachable!("the state machine should never be in the transition state")
            }
        }
    }

    /// Returns a mutable slice with all the available space to write to.
    ///
    /// After writing to this, call [Self::fill] with the number of bytes written.
    #[inline]
    pub fn space(&mut self) -> &mut [u8] {
        if self.buffer.available_space() == 0 {
            self.buffer.shift();
        }
        self.buffer.space()
    }

    /// After having written data to [Self::space], call this to indicate how
    /// many bytes were written.
    ///
    /// If this is called with zero, it indicates eof
    #[inline]
    pub fn fill(&mut self, count: usize) -> usize {
        if count == 0 {
            self.eof = true;
        }
        self.buffer.fill(count)
    }
}

enum AnyDecompressor {
    Store(store_dec::StoreDec),
    #[cfg(feature = "deflate")]
    Deflate(deflate_dec::DeflateDec),
}

#[derive(Default, Debug)]
pub struct DecompressOutcome {
    /// Number of bytes read from input
    pub bytes_read: usize,

    /// Number of bytes written to output
    pub bytes_written: usize,
}

trait Decompressor {
    #[inline]
    fn decompress(&mut self, in_buf: &[u8], out_buf: &mut [u8])
        -> Result<DecompressOutcome, Error>;
}

impl AnyDecompressor {
    fn new(method: Method) -> Result<Self, Error> {
        let dec = match method {
            Method::Store => Self::Store(Default::default()),

            #[cfg(feature = "deflate")]
            Method::Deflate => Self::Deflate(Default::default()),
            #[cfg(not(feature = "deflate"))]
            Method::Deflate => {
                let err = Error::Unsupported(UnsupportedError::MethodNotEnabled(method));
                return Err(err);
            }

            _ => {
                let err = Error::Unsupported(UnsupportedError::MethodNotSupported(method));
                return Err(err);
            }
        };
        Ok(dec)
    }

    #[inline]
    fn decompress(&mut self, in_buf: &[u8], out: &mut [u8]) -> Result<DecompressOutcome, Error> {
        /// forward to the appropriate decompressor
        match self {
            Self::Store(dec) => dec.decompress(in_buf, out),
            #[cfg(feature = "deflate")]
            Self::Deflate(dec) => dec.decompress(in_buf, out),
        }
    }
}
