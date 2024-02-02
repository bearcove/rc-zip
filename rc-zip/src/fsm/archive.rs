use super::FsmResult;
use crate::{
    encoding::Encoding, Archive, DirectoryHeader, EndOfCentralDirectory,
    EndOfCentralDirectory64Locator, EndOfCentralDirectory64Record, EndOfCentralDirectoryRecord,
    Error, FormatError, Located, StoredEntry,
};

use tracing::trace;
use winnow::{
    error::ErrMode,
    stream::{AsBytes, Offset},
    Parser, Partial,
};

/// [ArchiveReader] parses a valid zip archive into an [Archive]. In particular, this struct finds
/// an end of central directory record, parses the entire central directory, detects text encoding,
/// and normalizes metadata.
pub struct ArchiveFsm {
    // Size of the entire zip file
    size: u64,
    state: State,
}

#[derive(Default)]
enum State {
    /// Finding and reading the end of central directory record
    ReadEocd { buffer: Buffer, haystack_size: u64 },

    /// Reading the zip64 end of central directory record.
    ReadEocd64Locator {
        buffer: Buffer,
        eocdr: Located<EndOfCentralDirectoryRecord>,
    },

    /// Reading the zip64 end of central directory record.
    ReadEocd64 {
        buffer: Buffer,
        eocdr64_offset: u64,
        eocdr: Located<EndOfCentralDirectoryRecord>,
    },

    /// Reading all headers from the central directory
    ReadCentralDirectory {
        buffer: Buffer,
        eocd: EndOfCentralDirectory,
        directory_headers: Vec<DirectoryHeader>,
    },

    /// Done!
    Done,

    #[default]
    Transitioning,
}

impl State {
    fn get_buffer_mut(&mut self) -> Option<&mut Buffer> {
        use State as S;
        match self {
            S::ReadEocd { ref mut buffer, .. } => Some(buffer),
            S::ReadEocd64Locator { ref mut buffer, .. } => Some(buffer),
            S::ReadEocd64 { ref mut buffer, .. } => Some(buffer),
            S::ReadCentralDirectory { ref mut buffer, .. } => Some(buffer),
            _ => None,
        }
    }

    fn expect_buffer_mut(&mut self) -> &mut Buffer {
        self.get_buffer_mut()
            .expect("called expect_buffer_mut() on invalid state")
    }
}

impl ArchiveFsm {
    /// This should be > 65KiB, because the section at the end of the
    /// file that we check for end of central directory record is 65KiB.
    const DEFAULT_BUFFER_SIZE: usize = 256 * 1024;

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
            state: State::ReadEocd {
                buffer: Buffer::with_capacity(Self::DEFAULT_BUFFER_SIZE),
                haystack_size,
            },
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
        use State as S;
        match self.state {
            S::ReadEocd {
                ref buffer,
                haystack_size,
            } => Some(buffer.read_offset(self.size - haystack_size)),
            S::ReadEocd64Locator {
                ref buffer,
                ref eocdr,
            } => {
                let length = EndOfCentralDirectory64Locator::LENGTH as u64;
                Some(buffer.read_offset(eocdr.offset - length))
            }
            S::ReadEocd64 {
                ref buffer,
                eocdr64_offset,
                ..
            } => Some(buffer.read_offset(eocdr64_offset)),
            S::ReadCentralDirectory {
                ref buffer,
                ref eocd,
                ..
            } => Some(buffer.read_offset(eocd.directory_offset())),
            S::Done { .. } => panic!("Called wants_read() on ArchiveReader in Done state"),
            S::Transitioning => unreachable!(),
        }
    }

    /// returns a mutable slice with all the available space to
    /// write to
    #[inline]
    pub fn space(&mut self) -> &mut [u8] {
        let buf = self.state.expect_buffer_mut();
        trace!(
            available_space = buf.available_space(),
            "space() | available_space"
        );
        if buf.available_space() == 0 {
            buf.shift();
        }
        buf.space()
    }

    /// after having written data to the buffer, use this function
    /// to indicate how many bytes were written
    ///
    /// if there is not enough available space, this function can call
    /// `shift()` to move the remaining data to the beginning of the
    /// buffer
    #[inline]
    pub fn fill(&mut self, count: usize) -> usize {
        self.state.expect_buffer_mut().fill(count)
    }

    /// Process buffered data
    ///
    /// Errors returned from process() are caused by invalid zip archives,
    /// unsupported format quirks, or implementation bugs - never I/O errors.
    ///
    /// A result of [FsmResult::Continue] indicates one should loop again,
    /// starting with [wants_read()](ArchiveReader::wants_read()).
    ///
    /// A result of [FsmResult::Done] contains the [Archive], and indicates that no
    /// method should ever be called again on this reader.
    pub fn process(&mut self) -> Result<FsmResult<Archive>, Error> {
        use State as S;
        match self.state {
            S::ReadEocd {
                ref mut buffer,
                haystack_size,
            } => {
                if buffer.read_bytes() < haystack_size {
                    trace!(
                        read_bytes = buffer.read_bytes(),
                        haystack_size,
                        "ReadEocd | need more data"
                    );
                    return Ok(FsmResult::Continue);
                }

                match {
                    let haystack = &buffer.data()[..haystack_size as usize];
                    EndOfCentralDirectoryRecord::find_in_block(haystack)
                } {
                    None => Err(FormatError::DirectoryEndSignatureNotFound.into()),
                    Some(mut eocdr) => {
                        trace!(
                            ?eocdr,
                            size = self.size,
                            "ReadEocd | found end of central directory record"
                        );
                        buffer.reset();
                        eocdr.offset += self.size - haystack_size;

                        if eocdr.offset < EndOfCentralDirectory64Locator::LENGTH as u64 {
                            // no room for an EOCD64 locator, definitely not a zip64 file
                            trace!(
                                offset = eocdr.offset,
                                eocd64locator_length = EndOfCentralDirectory64Locator::LENGTH,
                                "no room for an EOCD64 locator, definitely not a zip64 file"
                            );
                            transition!(self.state => (S::ReadEocd { mut buffer, .. }) {
                                buffer.reset();
                                S::ReadCentralDirectory {
                                    buffer,
                                    eocd: EndOfCentralDirectory::new(self.size, eocdr, None)?,
                                    directory_headers: vec![],
                                }
                            });
                            Ok(FsmResult::Continue)
                        } else {
                            trace!("ReadEocd | transition to ReadEocd64Locator");
                            transition!(self.state => (S::ReadEocd { mut buffer, .. }) {
                                buffer.reset();
                                S::ReadEocd64Locator { buffer, eocdr }
                            });
                            Ok(FsmResult::Continue)
                        }
                    }
                }
            }
            S::ReadEocd64Locator { ref mut buffer, .. } => {
                let input = Partial::new(buffer.data());
                match EndOfCentralDirectory64Locator::parser.parse_peek(input) {
                    Err(ErrMode::Incomplete(_)) => {
                        // need more data
                        Ok(FsmResult::Continue)
                    }
                    Err(ErrMode::Backtrack(_)) | Err(ErrMode::Cut(_)) => {
                        // we don't have a zip64 end of central directory locator - that's ok!
                        trace!("ReadEocd64Locator | no zip64 end of central directory locator");
                        trace!("ReadEocd64Locator | data we got: {:02x?}", buffer.data());
                        transition!(self.state => (S::ReadEocd64Locator { mut buffer, eocdr }) {
                            buffer.reset();
                            S::ReadCentralDirectory {
                                buffer,
                                eocd: EndOfCentralDirectory::new(self.size, eocdr, None)?,
                                directory_headers: vec![],
                            }
                        });
                        Ok(FsmResult::Continue)
                    }
                    Ok((_, locator)) => {
                        trace!(
                            ?locator,
                            "ReadEocd64Locator | found zip64 end of central directory locator"
                        );
                        transition!(self.state => (S::ReadEocd64Locator { mut buffer, eocdr }) {
                            buffer.reset();
                            S::ReadEocd64 {
                                buffer,
                                eocdr64_offset: locator.directory_offset,
                                eocdr,
                            }
                        });
                        Ok(FsmResult::Continue)
                    }
                }
            }
            S::ReadEocd64 { ref mut buffer, .. } => {
                let input = Partial::new(buffer.data());
                match EndOfCentralDirectory64Record::parser.parse_peek(input) {
                    Err(ErrMode::Incomplete(_)) => {
                        // need more data
                        Ok(FsmResult::Continue)
                    }
                    Err(ErrMode::Backtrack(_)) | Err(ErrMode::Cut(_)) => {
                        // at this point, we really expected to have a zip64 end
                        // of central directory record, so, we want to propagate
                        // that error.
                        Err(FormatError::Directory64EndRecordInvalid.into())
                    }
                    Ok((_, eocdr64)) => {
                        transition!(self.state => (S::ReadEocd64 { mut buffer, eocdr, eocdr64_offset }) {
                            buffer.reset();
                            S::ReadCentralDirectory {
                                buffer,
                                eocd: EndOfCentralDirectory::new(self.size, eocdr, Some(Located {
                                    offset: eocdr64_offset,
                                    inner: eocdr64
                                }))?,
                                directory_headers: vec![],
                            }
                        });
                        Ok(FsmResult::Continue)
                    }
                }
            }
            S::ReadCentralDirectory {
                ref mut buffer,
                ref eocd,
                ref mut directory_headers,
            } => {
                trace!(
                    "ReadCentralDirectory | process(), available: {}",
                    buffer.available_data()
                );
                let mut input = Partial::new(buffer.data());
                trace!(
                    initial_offset = input.as_bytes().offset_from(&buffer.data()),
                    initial_len = input.len(),
                    "initial offset & len"
                );
                'read_headers: while !input.is_empty() {
                    match DirectoryHeader::parser.parse_next(&mut input) {
                        Ok(dh) => {
                            trace!(
                                input_empty_now = input.is_empty(),
                                offset = input.as_bytes().offset_from(&buffer.data()),
                                len = input.len(),
                                "ReadCentralDirectory | parsed directory header"
                            );
                            directory_headers.push(dh);
                        }
                        Err(ErrMode::Incomplete(_needed)) => {
                            // need more data to read the full header
                            trace!("ReadCentralDirectory | incomplete!");
                            break 'read_headers;
                        }
                        Err(ErrMode::Backtrack(_err)) | Err(ErrMode::Cut(_err)) => {
                            // this is the normal end condition when reading
                            // the central directory (due to 65536-entries non-zip64 files)
                            // let's just check a few numbers first.

                            // only compare 16 bits here
                            let expected_records = directory_headers.len() as u16;
                            let actual_records = eocd.directory_records() as u16;

                            if expected_records == actual_records {
                                let mut detectorng = chardetng::EncodingDetector::new();
                                let mut all_utf8 = true;
                                let mut had_suspicious_chars_for_cp437 = false;

                                {
                                    let max_feed: usize = 4096;
                                    let mut total_fed: usize = 0;
                                    let mut feed = |slice: &[u8]| {
                                        detectorng.feed(slice, false);
                                        for b in slice {
                                            if (0xB0..=0xDF).contains(b) {
                                                // those are, like, box drawing characters
                                                had_suspicious_chars_for_cp437 = true;
                                            }
                                        }

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
                                        let encoding = detectorng.guess(None, true);
                                        if encoding == encoding_rs::SHIFT_JIS {
                                            // well hold on, sometimes Codepage 437 is detected as
                                            // Shift-JIS by chardetng. If we have any characters
                                            // that aren't valid DOS file names, then okay it's probably
                                            // Shift-JIS. Otherwise, assume it's CP437.
                                            if had_suspicious_chars_for_cp437 {
                                                Encoding::ShiftJis
                                            } else {
                                                Encoding::Cp437
                                            }
                                        } else if encoding == encoding_rs::UTF_8 {
                                            Encoding::Utf8
                                        } else {
                                            Encoding::Cp437
                                        }
                                    }
                                };

                                let is_zip64 = eocd.dir64.is_some();
                                let global_offset = eocd.global_offset as u64;
                                let entries: Result<Vec<StoredEntry>, Error> = directory_headers
                                    .iter()
                                    .map(|x| x.as_stored_entry(is_zip64, encoding, global_offset))
                                    .collect();
                                let entries = entries?;

                                let mut comment: Option<String> = None;
                                if !eocd.comment().0.is_empty() {
                                    comment = Some(encoding.decode(&eocd.comment().0)?);
                                }

                                self.state = S::Done;
                                return Ok(FsmResult::Done(Archive {
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
                    }
                }
                let consumed = input.as_bytes().offset_from(&buffer.data());
                tracing::trace!(%consumed, "ReadCentralDirectory total consumed");
                buffer.consume(consumed);

                // need more data
                Ok(FsmResult::Continue)
            }
            S::Done { .. } => panic!("Called process() on ArchiveReader in Done state"),
            S::Transitioning => unreachable!(),
        }
    }
}

/// A wrapper around [oval::Buffer] that keeps track of how many bytes we've read since
/// initialization or the last reset.
pub(crate) struct Buffer {
    pub(crate) buffer: oval::Buffer,
    pub(crate) read_bytes: u64,
}

impl Buffer {
    /// creates a new buffer with the specified capacity
    pub(crate) fn with_capacity(size: usize) -> Self {
        Self {
            buffer: oval::Buffer::with_capacity(size),
            read_bytes: 0,
        }
    }

    /// resets the buffer (so that data() returns an empty slice,
    /// and space() returns the full capacity), along with th e
    /// read bytes counter.
    pub(crate) fn reset(&mut self) {
        self.read_bytes = 0;
        self.buffer.reset();
    }

    /// returns the number of read bytes since the last reset
    #[inline]
    pub(crate) fn read_bytes(&self) -> u64 {
        self.read_bytes
    }

    /// returns a slice with all the available data
    #[inline]
    pub(crate) fn data(&self) -> &[u8] {
        self.buffer.data()
    }

    /// returns how much data can be read from the buffer
    #[inline]
    pub(crate) fn available_data(&self) -> usize {
        self.buffer.available_data()
    }

    /// returns how much free space is available to write to
    #[inline]
    pub fn available_space(&self) -> usize {
        self.buffer.available_space()
    }

    /// returns a mutable slice with all the available space to
    /// write to
    #[inline]
    pub(crate) fn space(&mut self) -> &mut [u8] {
        self.buffer.space()
    }

    /// moves the data at the beginning of the buffer
    ///
    /// if the position was more than 0, it is now 0
    #[inline]
    pub fn shift(&mut self) {
        self.buffer.shift()
    }

    /// after having written data to the buffer, use this function
    /// to indicate how many bytes were written
    ///
    /// if there is not enough available space, this function can call
    /// `shift()` to move the remaining data to the beginning of the
    /// buffer
    #[inline]
    pub(crate) fn fill(&mut self, count: usize) -> usize {
        let n = self.buffer.fill(count);
        self.read_bytes += n as u64;
        n
    }

    /// advances the position tracker
    ///
    /// if the position gets past the buffer's half,
    /// this will call `shift()` to move the remaining data
    /// to the beginning of the buffer
    #[inline]
    pub(crate) fn consume(&mut self, size: usize) {
        self.buffer.consume(size);
    }

    /// computes an absolute offset, given an offset relative
    /// to the current read position
    pub(crate) fn read_offset(&self, offset: u64) -> u64 {
        self.read_bytes + offset
    }
}
