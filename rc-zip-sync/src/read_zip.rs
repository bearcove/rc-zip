use std::io::Read;

use rc_zip::{
    fsm::{ArchiveFsm, EntryFsm, FsmResult},
    Archive, Entry, Error,
};
use tracing::trace;

use crate::entry_reader::EntryReader;
use crate::streaming_entry_reader::StreamingEntryReader;

/// A trait for reading something as a zip archive
///
/// See also [ReadZip].
pub trait ReadZipWithSize {
    /// Reads self as a zip archive.
    fn read_zip_with_size(&self, size: u64) -> Result<Archive, Error>;
}

/// A trait for reading something as a zip archive when we can tell size from
/// self.
///
/// See also [ReadZipWithSize].
pub trait ReadZip {
    /// Reads self as a zip archive.
    fn read_zip(&self) -> Result<Archive, Error>;
}

struct CursorState<'a, F: HasCursor + 'a> {
    cursor: <F as HasCursor>::Cursor<'a>,
    offset: u64,
}

impl<'a, F: HasCursor + 'a> CursorState<'a, F> {
    /// Constructs a cursor only _after_ doing a bounds check with `offset` and `size`
    fn try_new(has_cursor: &'a F, offset: u64, size: u64) -> Result<Self, Error> {
        if offset > size {
            return Err(std::io::Error::other(format!(
                "archive tried reading beyond zip archive end. {offset} goes beyond {size}"
            ))
            .into());
        }
        let cursor = has_cursor.cursor_at(offset);
        Ok(Self { cursor, offset })
    }
}

impl<F> ReadZipWithSize for F
where
    F: HasCursor,
{
    fn read_zip_with_size(&self, size: u64) -> Result<Archive, Error> {
        let mut cstate: Option<CursorState<'_, F>> = None;

        let mut fsm = ArchiveFsm::new(size);
        loop {
            if let Some(offset) = fsm.wants_read() {
                trace!(%offset, "read_zip_with_size: wants_read, space len = {}", fsm.space().len());

                let mut cstate_next = match cstate.take() {
                    // all good, re-using
                    Some(cstate) if cstate.offset == offset => cstate,
                    Some(cstate) => {
                        trace!(%offset, %cstate.offset, "read_zip_with_size: making new cursor (had wrong offset)");
                        CursorState::try_new(self, offset, size)?
                    }
                    None => {
                        trace!(%offset, "read_zip_with_size: making new cursor (had none)");
                        CursorState::try_new(self, offset, size)?
                    }
                };

                match cstate_next.cursor.read(fsm.space()) {
                    Ok(read_bytes) => {
                        cstate_next.offset += read_bytes as u64;
                        cstate = Some(cstate_next);

                        trace!(%read_bytes, "read_zip_with_size: read");
                        if read_bytes == 0 {
                            return Err(Error::IO(std::io::ErrorKind::UnexpectedEof.into()));
                        }
                        fsm.fill(read_bytes);
                    }
                    Err(err) => return Err(Error::IO(err)),
                }
            }

            fsm = match fsm.process()? {
                FsmResult::Done(archive) => {
                    trace!("read_zip_with_size: done");
                    return Ok(archive);
                }
                FsmResult::Continue(fsm) => fsm,
            }
        }
    }
}

impl ReadZip for [u8] {
    fn read_zip(&self) -> Result<Archive, Error> {
        self.read_zip_with_size(self.len() as u64)
    }
}

/// A sliceable I/O resource: we can ask for a [Read] at a given offset.
pub trait HasCursor {
    /// The type of [Read] returned by [HasCursor::cursor_at].
    type Cursor<'a>: Read + 'a
    where
        Self: 'a;

    /// Returns a [Read] at the given offset.
    fn cursor_at(&self, offset: u64) -> Self::Cursor<'_>;
    /// Returns a [`EntryReader`] for the given [`Entry`].
    fn reader_at(&self, entry: &Entry) -> EntryReader<Self::Cursor<'_>> {
        EntryReader::new(entry, self.cursor_at(entry.header_offset))
    }
    /// Reads the entire entry into a vector.
    fn bytes_at(&self, entr: &Entry) -> std::io::Result<Vec<u8>> {
        let size = entr.uncompressed_size.try_into().unwrap_or(0);
        let mut v = Vec::with_capacity(size);
        self.reader_at(entr).read_to_end(&mut v)?;
        Ok(v)
    }
}

impl HasCursor for &[u8] {
    type Cursor<'a>
        = &'a [u8]
    where
        Self: 'a;

    fn cursor_at(&self, offset: u64) -> Self::Cursor<'_> {
        &self[offset.try_into().unwrap()..]
    }
}

impl HasCursor for Vec<u8> {
    type Cursor<'a>
        = &'a [u8]
    where
        Self: 'a;

    fn cursor_at(&self, offset: u64) -> Self::Cursor<'_> {
        &self[offset.try_into().unwrap()..]
    }
}

#[cfg(feature = "file")]
impl HasCursor for std::fs::File {
    type Cursor<'a>
        = positioned_io::Cursor<&'a std::fs::File>
    where
        Self: 'a;

    fn cursor_at(&self, offset: u64) -> Self::Cursor<'_> {
        positioned_io::Cursor::new_pos(self, offset)
    }
}

#[cfg(feature = "file")]
impl ReadZip for std::fs::File {
    fn read_zip(&self) -> Result<Archive, Error> {
        let size = self.metadata()?.len();
        self.read_zip_with_size(size)
    }
}

/// Allows reading zip entries in a streaming fashion, without seeking,
/// based only on local headers. THIS IS NOT RECOMMENDED, as correctly
/// reading zip files requires reading the central directory (located at
/// the end of the file).
pub trait ReadZipStreaming<R>
where
    R: Read,
{
    /// Get the first zip entry from the stream as a [StreamingEntryReader].
    ///
    /// See the trait's documentation for why using this is
    /// generally a bad idea: you might want to use [ReadZip] or
    /// [ReadZipWithSize] instead.
    fn stream_zip_entries_throwing_caution_to_the_wind(
        self,
    ) -> Result<StreamingEntryReader<R>, Error>;
}

impl<R> ReadZipStreaming<R> for R
where
    R: Read,
{
    fn stream_zip_entries_throwing_caution_to_the_wind(
        mut self,
    ) -> Result<StreamingEntryReader<Self>, Error> {
        let mut fsm = EntryFsm::new(None, None);

        loop {
            if fsm.wants_read() {
                let n = self.read(fsm.space())?;
                trace!("read {} bytes into buf for first zip entry", n);
                fsm.fill(n);
            }

            if let Some(entry) = fsm.process_till_header()? {
                let entry = entry.clone();
                return Ok(StreamingEntryReader::new(fsm, entry, self));
            }
        }
    }
}
