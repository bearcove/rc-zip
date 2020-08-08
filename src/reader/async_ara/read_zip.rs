use crate::{
    reader::{ArchiveReader, ArchiveReaderResult},
    Archive, Error,
};
use ara::ReadAt;
use async_trait::async_trait;
use std::io::Cursor;

#[async_trait(?Send)]
pub trait AsyncReadZip {
    async fn read_zip(&self) -> Result<Archive, Error>;
}

#[async_trait(?Send)]
impl<T> AsyncReadZip for T
where
    T: ReadAt,
{
    async fn read_zip(&self) -> Result<Archive, Error> {
        let mut buf = vec![0u8; 1024];

        let mut ar = ArchiveReader::new(self.len());
        let archive = loop {
            if let Some(offset) = ar.wants_read() {
                let n = self.read_at(offset, &mut buf[..]).await?;
                let mut cursor = Cursor::new(&buf[..n]);
                ar.read(&mut cursor)?;
            }

            match ar.process()? {
                ArchiveReaderResult::Continue => continue,
                ArchiveReaderResult::Done(archive) => break archive,
            }
        };

        Ok(archive)
    }
}
