use crate::{
    error::Error,
    format::Archive,
    reader::{sync::EntryReader, ArchiveReader, ArchiveReaderResult},
};
use positioned_io::{Cursor, ReadAt, Size};
use std::{fmt, fs::File, io::Read, ops::Deref};

/// A trait for reading something as a zip archive (blocking I/O model)
///
/// See also [ReadZip].
pub trait ReadZipWithSize {
    /// Reads self as a zip archive.
    ///
    /// This functions blocks until the entire archive has been read.
    /// It is not compatible with non-blocking or async I/O.
    fn read_zip_with_size(&self, size: u64) -> Result<SyncArchive<'_>, Error>;
}

/// A trait for reading something as a zip archive (blocking I/O model),
/// when we can tell size from self.
///
/// See also [ReadZipWithSize].
pub trait ReadZip {
    /// Reads self as a zip archive.
    ///
    /// This functions blocks until the entire archive has been read.
    /// It is not compatible with non-blocking or async I/O.
    fn read_zip(&self) -> Result<SyncArchive<'_>, Error>;
}

impl<T> ReadZipWithSize for T
where
    T: ReadAt,
{
    fn read_zip_with_size(&self, size: u64) -> Result<SyncArchive<'_>, Error> {
        let mut ar = ArchiveReader::new(size);
        loop {
            if let Some(offset) = ar.wants_read() {
                match ar.read(&mut Cursor::new_pos(&self, offset)) {
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

impl ReadZip for Vec<u8> {
    fn read_zip(&self) -> Result<SyncArchive<'_>, Error> {
        self.read_zip_with_size(self.len() as u64)
    }
}

#[cfg(feature = "file")]
impl ReadZip for File {
    fn read_zip(&self) -> Result<SyncArchive<'_>, Error> {
        let size = self.size()?.ok_or(Error::UnknownSize)?;
        self.read_zip_with_size(size)
    }
}

pub struct SyncArchive<'a> {
    file: &'a dyn ReadAt,
    archive: Archive,
}

impl fmt::Debug for SyncArchive<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SyncArchive")
            .field("archive", &self.archive)
            .finish_non_exhaustive()
    }
}

impl Deref for SyncArchive<'_> {
    type Target = Archive;

    fn deref(&self) -> &Self::Target {
        &self.archive
    }
}

impl SyncArchive<'_> {
    /// Iterate over all files in this zip, read from the central directory.
    pub fn entries(&self) -> impl Iterator<Item = SyncStoredEntry<'_>> {
        self.archive.entries().map(move |entry| SyncStoredEntry {
            file: self.file,
            entry,
        })
    }

    /// Attempts to look up an entry by name. This is usually a bad idea,
    /// as names aren't necessarily normalized in zip archives.
    pub fn by_name<N: AsRef<str>>(&self, name: N) -> Option<SyncStoredEntry<'_>> {
        self.entries
            .iter()
            .find(|&x| x.name() == name.as_ref())
            .map(|entry| SyncStoredEntry {
                file: self.file,
                entry,
            })
    }
}

pub struct SyncStoredEntry<'a> {
    file: &'a dyn ReadAt,
    entry: &'a crate::StoredEntry,
}

impl Deref for SyncStoredEntry<'_> {
    type Target = crate::StoredEntry;

    fn deref(&self) -> &Self::Target {
        self.entry
    }
}

impl SyncStoredEntry<'_> {
    /// Returns a reader for the entry.
    pub fn reader(&self) -> impl Read + '_ {
        EntryReader::new(self.entry, |offset| Cursor::new_pos(self.file, offset))
    }

    /// Reads the entire entry into a vector.
    pub fn bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut v = Vec::new();
        self.reader().read_to_end(&mut v)?;
        Ok(v)
    }
}
