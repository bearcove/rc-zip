use crate::{
    error::Error,
    format::Archive,
    reader::{sync::EntryReader, ArchiveReader, ArchiveReaderResult},
};
use std::{io::Read, ops::Deref};

/// A trait for reading something as a zip archive (blocking I/O model)
///
/// See also [ReadZip].
pub trait ReadZipWithSize {
    type File: HasCursor;

    /// Reads self as a zip archive.
    ///
    /// This functions blocks until the entire archive has been read.
    /// It is not compatible with non-blocking or async I/O.
    fn read_zip_with_size(&self, size: u64) -> Result<SyncArchive<'_, Self::File>, Error>;
}

/// A trait for reading something as a zip archive (blocking I/O model),
/// when we can tell size from self.
///
/// See also [ReadZipWithSize].
pub trait ReadZip {
    type File: HasCursor;

    /// Reads self as a zip archive.
    ///
    /// This functions blocks until the entire archive has been read.
    /// It is not compatible with non-blocking or async I/O.
    fn read_zip(&self) -> Result<SyncArchive<'_, Self::File>, Error>;
}

impl<F> ReadZipWithSize for F
where
    F: HasCursor,
{
    type File = F;

    fn read_zip_with_size(&self, size: u64) -> Result<SyncArchive<'_, F>, Error> {
        let mut ar = ArchiveReader::new(size);
        loop {
            if let Some(offset) = ar.wants_read() {
                match ar.read(&mut self.cursor_at(offset)) {
                    Ok(read_bytes) => {
                        if read_bytes == 0 {
                            return Err(Error::IO(std::io::ErrorKind::UnexpectedEof.into()));
                        }
                    }
                    Err(err) => return Err(Error::IO(err)),
                }
            }

            match ar.process()? {
                ArchiveReaderResult::Done(archive) => {
                    return Ok(SyncArchive {
                        file: self,
                        archive,
                    })
                }
                ArchiveReaderResult::Continue => {}
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
    pub fn entries(&self) -> impl Iterator<Item = SyncStoredEntry<'_, F>> {
        self.archive.entries().map(move |entry| SyncStoredEntry {
            file: self.file,
            entry,
        })
    }

    /// Attempts to look up an entry by name. This is usually a bad idea,
    /// as names aren't necessarily normalized in zip archives.
    pub fn by_name<N: AsRef<str>>(&self, name: N) -> Option<SyncStoredEntry<'_, F>> {
        self.entries
            .iter()
            .find(|&x| x.name() == name.as_ref())
            .map(|entry| SyncStoredEntry {
                file: self.file,
                entry,
            })
    }
}

pub struct SyncStoredEntry<'a, F> {
    file: &'a F,
    entry: &'a crate::StoredEntry,
}

impl<F> Deref for SyncStoredEntry<'_, F> {
    type Target = crate::StoredEntry;

    fn deref(&self) -> &Self::Target {
        self.entry
    }
}

impl<'a, F> SyncStoredEntry<'a, F>
where
    F: HasCursor,
{
    /// Returns a reader for the entry.
    pub fn reader(&self) -> EntryReader<<F as HasCursor>::Cursor<'a>> {
        tracing::trace!("Creating EntryReader");
        EntryReader::new(self.entry, |offset| self.file.cursor_at(offset))
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
