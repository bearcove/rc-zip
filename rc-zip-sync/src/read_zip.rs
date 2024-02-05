use rc_zip::parse::Entry;
use rc_zip::{
    error::Error,
    fsm::{ArchiveFsm, FsmResult},
    parse::{Archive, LocalFileHeader},
};
use tracing::trace;
use winnow::{error::ErrMode, Parser, Partial};

use crate::entry_reader::EntryReader;
use crate::streaming_entry_reader::StreamingEntryReader;
use std::{io::Read, ops::Deref};

/// A trait for reading something as a zip archive
///
/// See also [ReadZip].
pub trait ReadZipWithSize {
    /// The type of the file to read from.
    type File: HasCursor;

    /// Reads self as a zip archive.
    fn read_zip_with_size(&self, size: u64) -> Result<SyncArchive<'_, Self::File>, Error>;
}

/// A trait for reading something as a zip archive when we can tell size from
/// self.
///
/// See also [ReadZipWithSize].
pub trait ReadZip {
    /// The type of the file to read from.
    type File: HasCursor;

    /// Reads self as a zip archive.
    fn read_zip(&self) -> Result<SyncArchive<'_, Self::File>, Error>;
}

impl<F> ReadZipWithSize for F
where
    F: HasCursor,
{
    type File = F;

    fn read_zip_with_size(&self, size: u64) -> Result<SyncArchive<'_, F>, Error> {
        trace!(%size, "read_zip_with_size");
        let mut fsm = ArchiveFsm::new(size);
        loop {
            if let Some(offset) = fsm.wants_read() {
                trace!(%offset, "read_zip_with_size: wants_read, space len = {}", fsm.space().len());
                match self.cursor_at(offset).read(fsm.space()) {
                    Ok(read_bytes) => {
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
                    return Ok(SyncArchive {
                        file: self,
                        archive,
                    });
                }
                FsmResult::Continue(fsm) => fsm,
            }
        }
    }
}

impl ReadZip for &[u8] {
    type File = Self;

    fn read_zip(&self) -> Result<SyncArchive<'_, Self::File>, Error> {
        self.read_zip_with_size(self.len() as u64)
    }
}

impl ReadZip for Vec<u8> {
    type File = Self;

    fn read_zip(&self) -> Result<SyncArchive<'_, Self::File>, Error> {
        self.read_zip_with_size(self.len() as u64)
    }
}

/// A zip archive, read synchronously from a file or other I/O resource.
///
/// This only contains metadata for the archive and its entries. Separate
/// readers can be created for arbitraries entries on-demand using
/// [SyncEntry::reader].
pub struct SyncArchive<'a, F>
where
    F: HasCursor,
{
    file: &'a F,
    archive: Archive,
}

impl<F> Deref for SyncArchive<'_, F>
where
    F: HasCursor,
{
    type Target = Archive;

    fn deref(&self) -> &Self::Target {
        &self.archive
    }
}

impl<F> SyncArchive<'_, F>
where
    F: HasCursor,
{
    /// Iterate over all files in this zip, read from the central directory.
    pub fn entries(&self) -> impl Iterator<Item = SyncEntry<'_, F>> {
        self.archive.entries().map(move |entry| SyncEntry {
            file: self.file,
            entry,
        })
    }

    /// Attempts to look up an entry by name. This is usually a bad idea,
    /// as names aren't necessarily normalized in zip archives.
    pub fn by_name<N: AsRef<str>>(&self, name: N) -> Option<SyncEntry<'_, F>> {
        self.archive
            .entries()
            .find(|&x| x.name == name.as_ref())
            .map(|entry| SyncEntry {
                file: self.file,
                entry,
            })
    }
}

/// A zip entry, read synchronously from a file or other I/O resource.
pub struct SyncEntry<'a, F> {
    file: &'a F,
    entry: &'a Entry,
}

impl<F> Deref for SyncEntry<'_, F> {
    type Target = Entry;

    fn deref(&self) -> &Self::Target {
        self.entry
    }
}

impl<'a, F> SyncEntry<'a, F>
where
    F: HasCursor,
{
    /// Returns a reader for the entry.
    pub fn reader(&self) -> impl Read + 'a {
        EntryReader::new(self.entry, self.file.cursor_at(self.entry.header_offset))
    }

    /// Reads the entire entry into a vector.
    pub fn bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut v = Vec::new();
        self.reader().read_to_end(&mut v)?;
        Ok(v)
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
}

impl HasCursor for &[u8] {
    type Cursor<'a> = &'a [u8]
    where
        Self: 'a;

    fn cursor_at(&self, offset: u64) -> Self::Cursor<'_> {
        &self[offset.try_into().unwrap()..]
    }
}

impl HasCursor for Vec<u8> {
    type Cursor<'a> = &'a [u8]
    where
        Self: 'a;

    fn cursor_at(&self, offset: u64) -> Self::Cursor<'_> {
        &self[offset.try_into().unwrap()..]
    }
}

#[cfg(feature = "file")]
impl HasCursor for std::fs::File {
    type Cursor<'a> = positioned_io::Cursor<&'a std::fs::File>
    where
        Self: 'a;

    fn cursor_at(&self, offset: u64) -> Self::Cursor<'_> {
        positioned_io::Cursor::new_pos(self, offset)
    }
}

#[cfg(feature = "file")]
impl ReadZip for std::fs::File {
    type File = Self;

    fn read_zip(&self) -> Result<SyncArchive<'_, Self>, Error> {
        let size = self.metadata()?.len();
        self.read_zip_with_size(size)
    }
}

/// Allows reading zip entries in a streaming fashion, without seeking,
/// based only on local headers. THIS IS NOT RECOMMENDED, as correctly
/// reading zip files requires reading the central directory (located at
/// the end of the file).
///
/// Using local headers only involves a lot of guesswork and is only really
/// useful if you have some level of control over your input.
pub trait ReadZipEntriesStreaming<R>
where
    R: Read,
{
    /// Get the first zip entry from the stream as a [StreamingEntryReader].
    ///
    /// See [ReadZipEntriesStreaming]'s documentation for why using this is
    /// generally a bad idea: you might want to use [ReadZip] or
    /// [ReadZipWithSize] instead.
    fn read_first_zip_entry_streaming(self) -> Result<StreamingEntryReader<R>, Error>;
}

impl<R> ReadZipEntriesStreaming<R> for R
where
    R: Read,
{
    fn read_first_zip_entry_streaming(mut self) -> Result<StreamingEntryReader<Self>, Error> {
        let mut buf = oval::Buffer::with_capacity(16 * 1024);
        let entry = loop {
            let n = self.read(buf.space())?;
            trace!("read {} bytes into buf for first zip entry", n);
            buf.fill(n);

            let mut input = Partial::new(buf.data());
            match LocalFileHeader::parser.parse_next(&mut input) {
                Ok(header) => {
                    break header.as_entry()?;
                }
                Err(ErrMode::Incomplete(_)) => continue,
                Err(e) => {
                    panic!("{e}")
                }
            }
        };

        Ok(StreamingEntryReader::new(buf, entry, self))
    }
}
