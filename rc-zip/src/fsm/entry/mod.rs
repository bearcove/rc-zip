use std::cmp;

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

#[cfg(feature = "bzip2")]
mod bzip2_dec;

#[cfg(feature = "lzma")]
mod lzma_dec;

#[cfg(feature = "zstd")]
mod zstd_dec;

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

        /// Amount of bytes we've fed to the decompressor
        compressed_bytes: u64,

        /// Amount of bytes the decompressor has produced
        uncompressed_bytes: u64,

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
            State::Transition => unreachable!(),
        }
    }

    /// Process the input and write the output to the given buffer
    ///
    /// This function will return `FsmResult::Continue` if it needs more input
    /// to continue, or if it needs more space to write to. It will return
    /// `FsmResult::Done` when all the input has been decompressed and all
    /// the output has been written.
    ///
    /// Also, after writing all the output, process will read the data
    /// descriptor (if any), and make sur the CRC32 hash and the uncompressed
    /// size match the expected values.
    pub fn process(
        mut self,
        out: &mut [u8],
    ) -> Result<FsmResult<(Self, DecompressOutcome), ()>, Error> {
        tracing::trace!(
            state = match &self.state {
                State::ReadLocalHeader => "ReadLocalHeader",
                State::ReadData { .. } => "ReadData",
                State::ReadDataDescriptor { .. } => "ReadDataDescriptor",
                State::Validate { .. } => "Validate",
                State::Transition => "Transition",
            },
            "process"
        );

        use State as S;
        match &mut self.state {
            S::ReadLocalHeader => {
                let mut input = Partial::new(self.buffer.data());
                match LocalFileHeaderRecord::parser.parse_next(&mut input) {
                    Ok(header) => {
                        let consumed = input.as_bytes().offset_from(&self.buffer.data());
                        tracing::trace!(local_file_header = ?header, consumed, "parsed local file header");
                        self.buffer.consume(consumed);
                        self.state = S::ReadData {
                            header,
                            compressed_bytes: 0,
                            uncompressed_bytes: 0,
                            hasher: crc32fast::Hasher::new(),
                            decompressor: AnyDecompressor::new(self.method, &self.entry)?,
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
                compressed_bytes,
                uncompressed_bytes,
                hasher,
                decompressor,
                ..
            } => {
                let in_buf = self.buffer.data();

                // don't feed the decompressor bytes beyond the entry's compressed size
                let in_buf_max_len = cmp::min(
                    in_buf.len(),
                    self.entry.compressed_size as usize - *compressed_bytes as usize,
                );
                let in_buf = &in_buf[..in_buf_max_len];

                let fed_bytes_after_this = *compressed_bytes + in_buf.len() as u64;

                let has_more_input = if fed_bytes_after_this == self.entry.compressed_size as _ {
                    HasMoreInput::No
                } else {
                    HasMoreInput::Yes
                };
                let outcome = decompressor.decompress(in_buf, out, has_more_input)?;
                trace!(
                    ?outcome,
                    compressed_bytes = *compressed_bytes,
                    uncompressed_bytes = *uncompressed_bytes,
                    eof = self.eof,
                    "decompressed"
                );
                self.buffer.consume(outcome.bytes_read);
                *compressed_bytes += outcome.bytes_read as u64;

                if outcome.bytes_written == 0 && self.eof {
                    // we're done, let's read the data descriptor (if there's one)
                    transition!(self.state => (S::ReadData { header, uncompressed_bytes, hasher, .. }) {
                        let metrics = EntryReadMetrics {
                            uncompressed_size: uncompressed_bytes,
                            crc32: hasher.finalize(),
                        };

                        if header.has_data_descriptor() {
                            S::ReadDataDescriptor { header, metrics }
                        } else {
                            S::Validate { header, metrics, descriptor: None }
                        }
                    });
                    return self.process(out);
                }

                // write the decompressed data to the hasher
                hasher.update(&out[..outcome.bytes_written]);
                // update the number of bytes we've decompressed
                *uncompressed_bytes += outcome.bytes_written as u64;

                trace!(
                    compressed_bytes = *compressed_bytes,
                    uncompressed_bytes = *uncompressed_bytes,
                    "updated hasher"
                );

                Ok(FsmResult::Continue((self, outcome)))
            }
            S::ReadDataDescriptor { .. } => {
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
                    Err(_e) => Err(Error::Format(FormatError::InvalidDataDescriptor)),
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
                    }));
                }

                if expected_crc32 != 0 && expected_crc32 != metrics.crc32 {
                    return Err(Error::Format(FormatError::WrongChecksum {
                        expected: expected_crc32,
                        actual: metrics.crc32,
                    }));
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
    Deflate(Box<deflate_dec::DeflateDec>),
    #[cfg(feature = "bzip2")]
    Bzip2(bzip2_dec::Bzip2Dec),
    #[cfg(feature = "lzma")]
    Lzma(Box<lzma_dec::LzmaDec>),
    #[cfg(feature = "zstd")]
    Zstd(zstd_dec::ZstdDec),
}

#[derive(Default, Debug)]
pub struct DecompressOutcome {
    /// Number of bytes read from input
    pub bytes_read: usize,

    /// Number of bytes written to output
    pub bytes_written: usize,
}

pub enum HasMoreInput {
    Yes,
    No,
}

trait Decompressor {
    fn decompress(
        &mut self,
        in_buf: &[u8],
        out: &mut [u8],
        has_more_input: HasMoreInput,
    ) -> Result<DecompressOutcome, Error>;
}

impl AnyDecompressor {
    fn new(method: Method, entry: &StoredEntryInner) -> Result<Self, Error> {
        let dec = match method {
            Method::Store => Self::Store(Default::default()),

            #[cfg(feature = "deflate")]
            Method::Deflate => Self::Deflate(Default::default()),
            #[cfg(not(feature = "deflate"))]
            Method::Deflate => {
                let err = Error::Unsupported(UnsupportedError::MethodNotEnabled(method));
                return Err(err);
            }

            #[cfg(feature = "bzip2")]
            Method::Bzip2 => Self::Bzip2(Default::default()),
            #[cfg(not(feature = "bzip2"))]
            Method::Bzip2 => {
                let err = Error::Unsupported(UnsupportedError::MethodNotEnabled(method));
                return Err(err);
            }

            #[cfg(feature = "lzma")]
            Method::Lzma => Self::Lzma(Box::new(lzma_dec::LzmaDec::new(entry.uncompressed_size))),
            #[cfg(not(feature = "lzma"))]
            Method::Lzma => {
                let err = Error::Unsupported(UnsupportedError::MethodNotEnabled(method));
                return Err(err);
            }

            #[cfg(feature = "zstd")]
            Method::Zstd => Self::Zstd(zstd_dec::ZstdDec::new()?),
            #[cfg(not(feature = "zstd"))]
            Method::Zstd => {
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
}

impl Decompressor for AnyDecompressor {
    #[inline]
    fn decompress(
        &mut self,
        in_buf: &[u8],
        out: &mut [u8],
        has_more_input: HasMoreInput,
    ) -> Result<DecompressOutcome, Error> {
        // forward to the appropriate decompressor
        match self {
            Self::Store(dec) => dec.decompress(in_buf, out, has_more_input),
            #[cfg(feature = "deflate")]
            Self::Deflate(dec) => dec.decompress(in_buf, out, has_more_input),
            #[cfg(feature = "bzip2")]
            Self::Bzip2(dec) => dec.decompress(in_buf, out, has_more_input),
            #[cfg(feature = "lzma")]
            Self::Lzma(dec) => dec.decompress(in_buf, out, has_more_input),
            #[cfg(feature = "zstd")]
            Self::Zstd(dec) => dec.decompress(in_buf, out, has_more_input),
        }
    }
}
