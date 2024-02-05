use std::{cmp, io, ops::Deref, pin::Pin, sync::Arc, task};

use futures::future::BoxFuture;
use positioned_io::{RandomAccessFile, ReadAt, Size};
use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};

use rc_zip::{
    error::Error,
    fsm::{ArchiveFsm, EntryFsm, FsmResult},
    parse::{Archive, Entry},
};
use tracing::trace;

use crate::{entry_reader::EntryReader, StreamingEntryReader};

/// A trait for reading something as a zip archive.
///
/// See also [ReadZipAsync].
pub trait ReadZipWithSize {
    /// The type of the file to read from.
    type File: HasCursor;

    /// Reads self as a zip archive.
    #[allow(async_fn_in_trait)]
    async fn read_zip_with_size(&self, size: u64) -> Result<ArchiveHandle<'_, Self::File>, Error>;
}

/// A zip archive, read asynchronously from a file or other I/O resource.
///
/// This only contains metadata for the archive and its entries. Separate
/// readers can be created for arbitraries entries on-demand using
/// [AsyncEntry::reader].
pub trait ReadZip {
    /// The type of the file to read from.
    type File: HasCursor;

    /// Reads self as a zip archive.
    #[allow(async_fn_in_trait)]
    async fn read_zip(&self) -> Result<ArchiveHandle<'_, Self::File>, Error>;
}

impl<F> ReadZipWithSize for F
where
    F: HasCursor,
{
    type File = F;

    async fn read_zip_with_size(&self, size: u64) -> Result<ArchiveHandle<'_, F>, Error> {
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
                    return Ok(ArchiveHandle {
                        file: self,
                        archive,
                    })
                }
                FsmResult::Continue(fsm) => fsm,
            }
        }
    }
}

impl ReadZip for &[u8] {
    type File = Self;

    async fn read_zip(&self) -> Result<ArchiveHandle<'_, Self::File>, Error> {
        self.read_zip_with_size(self.len() as u64).await
    }
}

impl ReadZip for Vec<u8> {
    type File = Self;

    async fn read_zip(&self) -> Result<ArchiveHandle<'_, Self::File>, Error> {
        self.read_zip_with_size(self.len() as u64).await
    }
}

impl ReadZip for Arc<RandomAccessFile> {
    type File = Self;

    async fn read_zip(&self) -> Result<ArchiveHandle<'_, Self::File>, Error> {
        let size = self.size()?.unwrap_or_default();
        self.read_zip_with_size(size).await
    }
}

/// A zip archive, read asynchronously from a file or other I/O resource.
pub struct ArchiveHandle<'a, F>
where
    F: HasCursor,
{
    file: &'a F,
    archive: Archive,
}

impl<F> Deref for ArchiveHandle<'_, F>
where
    F: HasCursor,
{
    type Target = Archive;

    fn deref(&self) -> &Self::Target {
        &self.archive
    }
}

impl<F> ArchiveHandle<'_, F>
where
    F: HasCursor,
{
    /// Iterate over all files in this zip, read from the central directory.
    pub fn entries(&self) -> impl Iterator<Item = EntryHandle<'_, F>> {
        self.archive.entries().map(move |entry| EntryHandle {
            file: self.file,
            entry,
        })
    }

    /// Attempts to look up an entry by name. This is usually a bad idea,
    /// as names aren't necessarily normalized in zip archives.
    pub fn by_name<N: AsRef<str>>(&self, name: N) -> Option<EntryHandle<'_, F>> {
        self.archive
            .entries()
            .find(|&x| x.name == name.as_ref())
            .map(|entry| EntryHandle {
                file: self.file,
                entry,
            })
    }
}

/// A single entry in a zip archive, read asynchronously from a file or other I/O resource.
pub struct EntryHandle<'a, F> {
    file: &'a F,
    entry: &'a Entry,
}

impl<F> Deref for EntryHandle<'_, F> {
    type Target = Entry;

    fn deref(&self) -> &Self::Target {
        self.entry
    }
}

impl<'a, F> EntryHandle<'a, F>
where
    F: HasCursor,
{
    /// Returns a reader for the entry.
    pub fn reader(&self) -> impl AsyncRead + Unpin + '_ {
        EntryReader::new(self.entry, |offset| self.file.cursor_at(offset))
    }

    /// Reads the entire entry into a vector.
    pub async fn bytes(&self) -> io::Result<Vec<u8>> {
        let mut v = Vec::new();
        self.reader().read_to_end(&mut v).await?;
        Ok(v)
    }
}

/// A sliceable I/O resource: we can ask for an [AsyncRead] at a given offset.
pub trait HasCursor {
    /// The type returned by [HasAsyncCursor::cursor_at].
    type Cursor<'a>: AsyncRead + Unpin + 'a
    where
        Self: 'a;

    /// Returns an [AsyncRead] at the given offset.
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

impl HasCursor for Arc<RandomAccessFile> {
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
                let read_len = cmp::min(buf.remaining(), core.inner_buf.len());
                let pos = self.pos;
                let fut = Box::pin(tokio::task::spawn_blocking(move || {
                    let read = core.file.read_at(pos, &mut core.inner_buf[..read_len]);
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

/// Allows reading zip entries in a streaming fashion, without seeking,
/// based only on local headers. THIS IS NOT RECOMMENDED, as correctly
/// reading zip files requires reading the central directory (located at
/// the end of the file).
pub trait ReadZipStreaming<R>
where
    R: AsyncRead,
{
    /// Get the first zip entry from the stream as a [StreamingEntryReader].
    ///
    /// See the trait's documentation for why using this is
    /// generally a bad idea: you might want to use [ReadZip] or
    /// [ReadZipWithSize] instead.
    #[allow(async_fn_in_trait)]
    async fn stream_zip_entries_throwing_caution_to_the_wind(
        self,
    ) -> Result<StreamingEntryReader<R>, Error>;
}

impl<R> ReadZipStreaming<R> for R
where
    R: AsyncRead + Unpin,
{
    async fn stream_zip_entries_throwing_caution_to_the_wind(
        mut self,
    ) -> Result<StreamingEntryReader<Self>, Error> {
        let mut fsm = EntryFsm::new(None, None);

        loop {
            if fsm.wants_read() {
                let n = self.read(fsm.space()).await?;
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
