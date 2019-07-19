use log::*;
use std::io::Read;

use crate::{encoding::Encoding, error::*, format::*};
use libflate::non_blocking::deflate;

mod buffer;
use self::buffer::{Buffer, ReadOp};

mod read_zip;
pub use self::read_zip::*;

use nom::Offset;

// Reference code for zip handling:
// https://github.com/itchio/arkive/blob/master/zip/reader.go

/// ArchiveReader parses a valid zip archive into an [Archive][]. In particular, this struct finds
/// an end of central directory record, parses the entire central directory, detects text encoding,
/// and normalizes metadata.
pub struct ArchiveReader {
    // Size of the entire zip file
    size: u64,
    state: ArchiveReaderState,

    buffer: Buffer,
}

#[derive(Debug)]
pub enum ArchiveReaderResult {
    /// Indicates that [ArchiveReader][] has work left, and the loop should continue.
    Continue,
    /// Indicates that [ArchiveReader][] is done reading the central directory,
    /// contains an [Archive][]. Calling any method after [process()](ArchiveReader::process()) has returned
    /// `Done` will panic.
    Done(Archive),
}

enum ArchiveReaderState {
    /// Used while transitioning because ownership rules are tough.
    Transitioning,

    /// Finding and reading the end of central directory record
    ReadEocd { haystack_size: u64 },

    /// Reading the zip64 end of central directory record.
    ReadEocd64Locator {
        eocdr: Located<EndOfCentralDirectoryRecord>,
    },

    /// Reading the zip64 end of central directory record.
    ReadEocd64 {
        eocdr64_offset: u64,
        eocdr: Located<EndOfCentralDirectoryRecord>,
    },

    /// Reading all headers from the central directory
    ReadCentralDirectory {
        eocd: EndOfCentralDirectory,
        directory_headers: Vec<DirectoryHeader>,
    },

    /// Done!
    Done,
}

macro_rules! transition {
    ($state: expr => ($pattern: pat) $body: expr) => {
        $state = if let $pattern = std::mem::replace(&mut $state, S::Transitioning) {
            $body
        } else {
            unreachable!()
        };
    };
}

impl ArchiveReader {
    /// Create a new archive reader with a specified file size.
    ///
    /// Actual reading of the file is performed by calling
    /// [wants_read()](ArchiveReader::wants_read()), [read()](ArchiveReader::read()) and
    /// [process()](ArchiveReader::process()) in a loop.
    pub fn new(size: u64) -> Self {
        let haystack_size: u64 = 65 * 1024;
        let haystack_size = if size < haystack_size {
            size
        } else {
            haystack_size
        };

        Self {
            size,
            state: ArchiveReaderState::ReadEocd { haystack_size },
            buffer: Buffer::with_capacity(128 * 1024), // 128KB buffer
        }
    }

    /// Returns whether or not this reader needs more data to continue.
    ///
    /// Returns `Some(offset)` if this reader needs to read some data from `offset`.
    /// In this case, [read()](ArchiveReader::read()) should be called with a [Read]
    /// at the correct offset.
    ///
    /// Returns `None` if the reader does not need data and [process()](ArchiveReader::process())
    /// can be called directly.
    pub fn wants_read(&self) -> Option<u64> {
        self.read_op().map(|op| self.buffer.read_offset(op))
    }

    fn read_op(&self) -> Option<ReadOp> {
        use ArchiveReaderState as S;
        match self.state {
            S::ReadEocd { haystack_size } => Some(ReadOp {
                offset: self.size - haystack_size,
            }),
            S::ReadEocd64Locator { ref eocdr } => {
                let length = EndOfCentralDirectory64Locator::LENGTH as u64;
                Some(ReadOp {
                    offset: eocdr.offset - length,
                })
            }
            S::ReadEocd64 { eocdr64_offset, .. } => Some(ReadOp {
                offset: eocdr64_offset,
            }),
            S::ReadCentralDirectory { ref eocd, .. } => Some(ReadOp {
                offset: eocd.directory_offset(),
            }),
            S::Done { .. } => panic!("Called wants_read() on ArchiveReader in Done state"),
            S::Transitioning => unreachable!(),
        }
    }

    /// Reads some data from `rd` into the reader's internal buffer.
    ///
    /// Any I/O errors will be returned.
    ///
    /// If successful, this returns the number of bytes read. On success,
    /// [process()](ArchiveReader::process()) should be called next.
    pub fn read(&mut self, rd: &mut Read) -> Result<usize, std::io::Error> {
        self.buffer.read(rd)
    }

    /// Process buffered data
    ///
    /// Errors returned from process() are caused by invalid zip archives,
    /// unsupported format quirks, or implementation bugs - never I/O errors.
    ///
    /// A result of [ArchiveReaderResult::Continue] indicates one should loop again,
    /// starting with [wants_read()](ArchiveReader::wants_read()).
    ///
    /// A result of [ArchiveReaderResult::Done] contains the [Archive], and indicates that no
    /// method should ever be called again on this reader.
    pub fn process(&mut self) -> Result<ArchiveReaderResult, Error> {
        use ArchiveReaderResult as R;
        use ArchiveReaderState as S;
        match self.state {
            S::ReadEocd { haystack_size } => {
                if self.buffer.read_bytes() < haystack_size {
                    return Ok(R::Continue);
                }

                match {
                    let haystack = &self.buffer.data()[..haystack_size as usize];
                    EndOfCentralDirectoryRecord::find_in_block(haystack)
                } {
                    None => Err(FormatError::DirectoryEndSignatureNotFound.into()),
                    Some(mut eocdr) => {
                        self.buffer.reset();
                        eocdr.offset += self.size - haystack_size;

                        if eocdr.offset < EndOfCentralDirectory64Locator::LENGTH as u64 {
                            // no room for an EOCD64 locator, definitely not a zip64 file
                            self.state = S::ReadCentralDirectory {
                                eocd: EndOfCentralDirectory::new(self.size, eocdr, None)?,
                                directory_headers: vec![],
                            };
                            Ok(R::Continue)
                        } else {
                            self.buffer.reset();
                            self.state = S::ReadEocd64Locator { eocdr };
                            Ok(R::Continue)
                        }
                    }
                }
            }
            S::ReadEocd64Locator { .. } => {
                match EndOfCentralDirectory64Locator::parse(self.buffer.data()) {
                    Err(nom::Err::Incomplete(_)) => {
                        // need more data
                        Ok(R::Continue)
                    }
                    Err(nom::Err::Error(_)) | Err(nom::Err::Failure(_)) => {
                        // we don't have a zip64 end of central directory locator - that's ok!
                        self.buffer.reset();
                        transition!(self.state => (S::ReadEocd64Locator {eocdr}) {
                            S::ReadCentralDirectory {
                                eocd: EndOfCentralDirectory::new(self.size, eocdr, None)?,
                                directory_headers: vec![],
                            }
                        });
                        Ok(R::Continue)
                    }
                    Ok((_, locator)) => {
                        self.buffer.reset();
                        transition!(self.state => (S::ReadEocd64Locator {eocdr}) {
                            S::ReadEocd64 {
                                eocdr64_offset: locator.directory_offset,
                                eocdr,
                            }
                        });
                        Ok(R::Continue)
                    }
                }
            }
            S::ReadEocd64 { .. } => {
                match EndOfCentralDirectory64Record::parse(self.buffer.data()) {
                    Err(nom::Err::Incomplete(_)) => {
                        // need more data
                        Ok(R::Continue)
                    }
                    Err(nom::Err::Error(_)) | Err(nom::Err::Failure(_)) => {
                        // at this point, we really expected to have a zip64 end
                        // of central directory record, so, we want to propagate
                        // that error.
                        Err(FormatError::Directory64EndRecordInvalid.into())
                    }
                    Ok((_, eocdr64)) => {
                        self.buffer.reset();
                        transition!(self.state => (S::ReadEocd64 { eocdr, eocdr64_offset }) {
                            S::ReadCentralDirectory {
                                eocd: EndOfCentralDirectory::new(self.size, eocdr, Some(Located {
                                    offset: eocdr64_offset,
                                    inner: eocdr64
                                }))?,
                                directory_headers: vec![],
                            }
                        });
                        Ok(R::Continue)
                    }
                }
            }
            S::ReadCentralDirectory {
                ref eocd,
                ref mut directory_headers,
            } => {
                debug!(
                    "ReadCentralDirectory | process(), available: {}",
                    self.buffer.available_data()
                );
                // FIXME: see https://github.com/rust-compress/rc-zip/issues/3
                'read_headers: while self.buffer.available_data()
                    >= DirectoryHeader::SIGNATURE_LENGTH
                {
                    match DirectoryHeader::parse(self.buffer.data()) {
                        Err(nom::Err::Incomplete(_needed)) => {
                            // need more data
                            break 'read_headers;
                        }
                        Err(nom::Err::Error(_err)) | Err(nom::Err::Failure(_err)) => {
                            let (_, kind) = _err;
                            debug!("nom error kind: {:#?}", kind);
                            match kind {
                                nom::error::ErrorKind::Eof => {
                                    // need more data
                                    break 'read_headers;
                                }
                                _ => {}
                            }

                            // this is the normal end condition when reading
                            // the central directory (due to 65536-entries non-zip64 files)
                            // let's just check a few numbers first.

                            // only compare 16 bits here
                            let expected_records = directory_headers.len() as u16;
                            let actual_records = eocd.directory_records() as u16;

                            if expected_records == actual_records {
                                let mut detector = chardet::UniversalDetector::new();
                                let mut all_utf8 = true;

                                {
                                    let max_feed: usize = 4096;
                                    let mut total_fed: usize = 0;
                                    let mut feed = |slice: &[u8]| {
                                        detector.feed(slice);
                                        total_fed += slice.len();
                                        total_fed < max_feed
                                    };

                                    'recognize_encoding: for fh in
                                        directory_headers.iter().filter(|fh| fh.is_non_utf8())
                                    {
                                        all_utf8 = false;
                                        if !feed(&fh.name.0) || !feed(&fh.comment.0) {
                                            break 'recognize_encoding;
                                        }
                                    }
                                }

                                let encoding = {
                                    if all_utf8 {
                                        Encoding::Utf8
                                    } else {
                                        let (charset, confidence, _language) = detector.close();
                                        let label = chardet::charset2encoding(&charset);
                                        debug!(
                                            "Detected charset {} with confidence {}",
                                            label, confidence
                                        );

                                        match label {
                                            "SHIFT_JIS" => Encoding::ShiftJis,
                                            "utf-8" => Encoding::Utf8,
                                            _ => Encoding::Cp437,
                                        }
                                    }
                                };

                                let is_zip64 = eocd.dir64.is_some();
                                let entries: Result<Vec<StoredEntry>, Error> = directory_headers
                                    .into_iter()
                                    .map(|x| x.as_stored_entry(is_zip64, encoding))
                                    .collect();
                                let entries = entries?;

                                let mut comment: Option<String> = None;
                                if !eocd.comment().0.is_empty() {
                                    comment = Some(encoding.decode(&eocd.comment().0)?);
                                }

                                self.state = S::Done;
                                return Ok(R::Done(Archive {
                                    size: self.size,
                                    comment,
                                    entries,
                                    encoding,
                                }));
                            } else {
                                // if we read the wrong number of directory entries,
                                // error out.
                                return Err(FormatError::InvalidCentralRecord {
                                    expected: expected_records,
                                    actual: actual_records,
                                }
                                .into());
                            }
                        }
                        Ok((remaining, dh)) => {
                            let consumed = self.buffer.data().offset(remaining);
                            drop(remaining);
                            self.buffer.consume(consumed);
                            directory_headers.push(dh);
                        }
                    }
                }

                // need more data
                return Ok(R::Continue);
            }
            S::Done { .. } => panic!("Called process() on ArchiveReader in Done state"),
            S::Transitioning => unreachable!(),
        }
    }
}

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
        decoder: deflate::Decoder<circular::Buffer>,
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

                            S::ReadData {
                                hasher: crc32fast::Hasher::new(),
                                uncompressed_size: 0,
                                decoder: deflate::Decoder::new(buffer),
                                header,
                                read_bytes,
                            }
                        });
                        self.read(buf)
                    }
                    Err(_e) => Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        Error::Format(FormatError::InvalidLocalHeader),
                    )),
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
                    Err(_e) => Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        Error::Format(FormatError::InvalidLocalHeader),
                    )),
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
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            Error::Format(FormatError::WrongChecksum {
                                expected: expected_crc32,
                                actual: metrics.crc32,
                            }),
                        ));
                    }
                }

                if expected_size != metrics.uncompressed_size {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        Error::Format(FormatError::WrongSize {
                            expected: expected_size,
                            actual: metrics.uncompressed_size,
                        }),
                    ));
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

pub struct EntryRead {}
