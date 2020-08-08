use crate::{
    error::Error,
    format::Archive,
    reader::{ArchiveReader, ArchiveReaderResult},
};
use positioned_io::{Cursor, ReadAt, Size};
use std::fs::File;

/// A trait for reading something as a zip archive (blocking I/O model)
///
/// See also [ReadZip].
pub trait ReadZipWithSize {
    /// Reads self as a zip archive.
    ///
    /// This functions blocks until the entire archive has been read.
    /// It is not compatible with non-blocking or async I/O.
    fn read_zip_with_size(&self, size: u64) -> Result<Archive, Error>;
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
    fn read_zip(&self) -> Result<Archive, Error>;
}

impl ReadZipWithSize for dyn ReadAt {
    fn read_zip_with_size(&self, size: u64) -> Result<Archive, Error> {
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
                ArchiveReaderResult::Done(archive) => return Ok(archive),
                ArchiveReaderResult::Continue => {}
            }
        }
    }
}

impl ReadZip for Vec<u8> {
    fn read_zip(&self) -> Result<Archive, Error> {
        ReadAt::read_zip_with_size(self, self.len() as u64)
    }
}

impl ReadZip for File {
    fn read_zip(&self) -> Result<Archive, Error> {
        let size = self.size()?.ok_or(Error::UnknownSize)?;
        ReadAt::read_zip_with_size(self, size)
    }
}
