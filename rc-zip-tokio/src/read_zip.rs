use std::{io, ops::Deref, pin::Pin, sync::Arc, task};

use futures::future::BoxFuture;
use positioned_io::{RandomAccessFile, ReadAt};
use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};

use rc_zip::{
    fsm::{ArchiveFsm, FsmResult},
    Archive, Error, StoredEntry,
};

use crate::entry_reader::AsyncEntryReader;

/// A trait for reading something as a zip archive (blocking I/O model)
///
/// See also [ReadZip].
pub trait AsyncReadZipWithSize {
    /// The type of the file to read from.
    type File: HasAsyncCursor;

    /// Reads self as a zip archive.
    ///
    /// This functions blocks until the entire archive has been read.
    /// It is not compatible with non-blocking or async I/O.
    #[allow(async_fn_in_trait)]
    async fn read_zip_with_size_async(
        &self,
        size: u64,
    ) -> Result<AsyncArchive<'_, Self::File>, Error>;
}

/// A trait for reading something as a zip archive (blocking I/O model),
/// when we can tell size from self.
///
/// See also [ReadZipWithSize].
pub trait AsyncReadZip {
    /// The type of the file to read from.
    type File: HasAsyncCursor;

    /// Reads self as a zip archive.
    ///
    /// This functions blocks until the entire archive has been read.
    /// It is not compatible with non-blocking or async I/O.
    #[allow(async_fn_in_trait)]
    async fn read_zip_async(&self) -> Result<AsyncArchive<'_, Self::File>, Error>;
}

impl<F> AsyncReadZipWithSize for F
where
    F: HasAsyncCursor,
{
    type File = F;

    async fn read_zip_with_size_async(&self, size: u64) -> Result<AsyncArchive<'_, F>, Error> {
        let mut fsm = ArchiveFsm::new(size);
        loop {
            if let Some(offset) = fsm.wants_read() {
                match self.cursor_at(offset).read(fsm.space()).await {
                    Ok(read_bytes) => {
                        if read_bytes == 0 {
                            return Err(Error::IO(io::ErrorKind::UnexpectedEof.into()));
                        }
                        fsm.fill(read_bytes);
                    }
                    Err(err) => return Err(Error::IO(err)),
                }
            }

            fsm = match fsm.process()? {
                FsmResult::Done(archive) => {
                    return Ok(AsyncArchive {
                        file: self,
                        archive,
                    })
                }
                FsmResult::Continue(fsm) => fsm,
            }
        }
    }
}

impl AsyncReadZip for &[u8] {
    type File = Self;

    async fn read_zip_async(&self) -> Result<AsyncArchive<'_, Self::File>, Error> {
        self.read_zip_with_size_async(self.len() as u64).await
    }
}

impl AsyncReadZip for Vec<u8> {
    type File = Self;

    async fn read_zip_async(&self) -> Result<AsyncArchive<'_, Self::File>, Error> {
        self.read_zip_with_size_async(self.len() as u64).await
    }
}

/// A zip archive, read asynchronously from a file or other I/O resource.
pub struct AsyncArchive<'a, F>
where
    F: HasAsyncCursor,
{
    file: &'a F,
    archive: Archive,
}

impl<F> Deref for AsyncArchive<'_, F>
where
    F: HasAsyncCursor,
{
    type Target = Archive;

    fn deref(&self) -> &Self::Target {
        &self.archive
    }
}

impl<F> AsyncArchive<'_, F>
where
    F: HasAsyncCursor,
{
    /// Iterate over all files in this zip, read from the central directory.
    pub fn entries(&self) -> impl Iterator<Item = AsyncStoredEntry<'_, F>> {
        self.archive.entries().map(move |entry| AsyncStoredEntry {
            file: self.file,
            entry,
        })
    }

    /// Attempts to look up an entry by name. This is usually a bad idea,
    /// as names aren't necessarily normalized in zip archives.
    pub fn by_name<N: AsRef<str>>(&self, name: N) -> Option<AsyncStoredEntry<'_, F>> {
        self.archive
            .entries()
            .find(|&x| x.name() == name.as_ref())
            .map(|entry| AsyncStoredEntry {
                file: self.file,
                entry,
            })
    }
}

/// A single entry in a zip archive, read asynchronously from a file or other I/O resource.
pub struct AsyncStoredEntry<'a, F> {
    file: &'a F,
    entry: &'a StoredEntry,
}

impl<F> Deref for AsyncStoredEntry<'_, F> {
    type Target = StoredEntry;

    fn deref(&self) -> &Self::Target {
        self.entry
    }
}

impl<'a, F> AsyncStoredEntry<'a, F>
where
    F: HasAsyncCursor,
{
    /// Returns a reader for the entry.
    pub fn reader(&self) -> impl AsyncRead + Unpin + '_ {
        tracing::trace!("Creating EntryReader");
        AsyncEntryReader::new(self.entry, |offset| self.file.cursor_at(offset))
    }

    /// Reads the entire entry into a vector.
    pub async fn bytes(&self) -> io::Result<Vec<u8>> {
        let mut v = Vec::new();
        self.reader().read_to_end(&mut v).await?;
        Ok(v)
    }
}

/// A sliceable I/O resource: we can ask for a [Read] at a given offset.
pub trait HasAsyncCursor {
    /// The type returned by [cursor_at].
    type Cursor<'a>: AsyncRead + Unpin + 'a
    where
        Self: 'a;

    /// Returns a [Read] at the given offset.
    fn cursor_at(&self, offset: u64) -> Self::Cursor<'_>;
}

impl HasAsyncCursor for &[u8] {
    type Cursor<'a> = &'a [u8]
    where
        Self: 'a;

    fn cursor_at(&self, offset: u64) -> Self::Cursor<'_> {
        &self[offset.try_into().unwrap()..]
    }
}

impl HasAsyncCursor for Vec<u8> {
    type Cursor<'a> = &'a [u8]
    where
        Self: 'a;

    fn cursor_at(&self, offset: u64) -> Self::Cursor<'_> {
        &self[offset.try_into().unwrap()..]
    }
}

impl HasAsyncCursor for Arc<RandomAccessFile> {
    type Cursor<'a> = AsyncRandomAccessFileCursor
    where
        Self: 'a;

    fn cursor_at(&self, offset: u64) -> Self::Cursor<'_> {
        AsyncRandomAccessFileCursor {
            pos: offset,
            state: ARAFCState::Idle(ARAFCCore {
                inner_buf: vec![0u8; 128 * 1024],
                file: self.clone(),
            }),
        }
    }
}

struct ARAFCCore {
    inner_buf: Vec<u8>,
    file: Arc<RandomAccessFile>,
}

type JoinResult<T> = Result<T, tokio::task::JoinError>;

#[derive(Default)]
enum ARAFCState {
    Idle(ARAFCCore),
    Reading {
        fut: BoxFuture<'static, JoinResult<(Result<usize, io::Error>, ARAFCCore)>>,
    },

    #[default]
    Transitioning,
}

/// A cursor for reading from a [RandomAccessFile] asynchronously.
pub struct AsyncRandomAccessFileCursor {
    pos: u64,
    state: ARAFCState,
}

impl AsyncRead for AsyncRandomAccessFileCursor {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> task::Poll<io::Result<()>> {
        match &mut self.state {
            ARAFCState::Idle { .. } => {
                let mut core = match std::mem::take(&mut self.state) {
                    ARAFCState::Idle(core) => core,
                    _ => unreachable!(),
                };
                let pos = self.pos;
                let fut = Box::pin(tokio::task::spawn_blocking(move || {
                    let read = core.file.read_at(pos, &mut core.inner_buf);
                    (read, core)
                }));
                self.state = ARAFCState::Reading { fut };
                self.poll_read(cx, buf)
            }
            ARAFCState::Reading { fut } => {
                let (read, core) = match fut.as_mut().poll(cx) {
                    task::Poll::Ready(Ok(r)) => r,
                    task::Poll::Ready(Err(e)) => {
                        return task::Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::Other,
                            e.to_string(),
                        )))
                    }
                    task::Poll::Pending => return task::Poll::Pending,
                };
                match read {
                    Ok(read) => {
                        self.pos += read as u64;
                        buf.put_slice(&core.inner_buf[..read]);
                        self.state = ARAFCState::Idle(core);
                        task::Poll::Ready(Ok(()))
                    }
                    Err(e) => task::Poll::Ready(Err(e)),
                }
            }
            ARAFCState::Transitioning => unreachable!(),
        }
    }
}
