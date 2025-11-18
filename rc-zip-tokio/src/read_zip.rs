use std::{
    cmp, io,
    ops::Deref,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures_util::future::BoxFuture;
use positioned_io::{RandomAccessFile, ReadAt, Size};
use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};

use rc_zip::{
    fsm::{ArchiveFsm, EntryFsm, FsmResult},
    Archive, Entry, Error,
};
use tracing::trace;

use crate::{entry_reader::EntryReader, StreamingEntryReader};

/// A trait for reading something as a zip archive.
///
/// See also [ReadZip].
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
/// [EntryHandle::reader].
pub trait ReadZip {
    /// The type of the file to read from.
    type File: HasCursor;

    /// Reads self as a zip archive.
    #[allow(async_fn_in_trait)]
    async fn read_zip(&self) -> Result<ArchiveHandle<'_, Self::File>, Error>;
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
    type File = F;

    async fn read_zip_with_size(&self, size: u64) -> Result<ArchiveHandle<'_, F>, Error> {
        let mut cstate: Option<CursorState<'_, F>> = None;

        let mut fsm = ArchiveFsm::new(size);
        loop {
            if let Some(offset) = fsm.wants_read() {
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

                match cstate_next.cursor.read(fsm.space()).await {
                    Ok(read_bytes) => {
                        cstate_next.offset += read_bytes as u64;
                        cstate = Some(cstate_next);

                        trace!(%read_bytes, "filling fsm");
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
    /// The type returned by [HasCursor::cursor_at].
    type Cursor<'a>: AsyncRead + Unpin + 'a
    where
        Self: 'a;

    /// Returns an [AsyncRead] at the given offset.
    fn cursor_at(&self, offset: u64) -> Self::Cursor<'_>;
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

impl HasCursor for Arc<RandomAccessFile> {
    type Cursor<'a>
        = AsyncRandomAccessFileCursor
    where
        Self: 'a;

    fn cursor_at(&self, offset: u64) -> Self::Cursor<'_> {
        AsyncRandomAccessFileCursor {
            state: ARAFCState::Idle(ARAFCCore {
                file_offset: offset,
                inner_buf: vec![0u8; 128 * 1024],
                // inner_buf: vec![0u8; 128],
                inner_buf_len: 0,
                inner_buf_offset: 0,
                file: self.clone(),
            }),
        }
    }
}

struct ARAFCCore {
    // offset we're reading from in the file
    file_offset: u64,

    // note: the length of this vec is the inner buffer capacity
    inner_buf: Vec<u8>,

    // the start of data we haven't returned put to caller buffets yet
    inner_buf_offset: usize,

    // the end of data we haven't returned put to caller buffets yet
    inner_buf_len: usize,

    file: Arc<RandomAccessFile>,
}

type JoinResult<T> = Result<T, tokio::task::JoinError>;

#[derive(Default)]
enum ARAFCState {
    Idle(ARAFCCore),
    Reading {
        fut: BoxFuture<'static, JoinResult<Result<ARAFCCore, io::Error>>>,
    },

    #[default]
    Transitioning,
}

/// A cursor for reading from a [RandomAccessFile] asynchronously.
pub struct AsyncRandomAccessFileCursor {
    state: ARAFCState,
}

impl AsyncRead for AsyncRandomAccessFileCursor {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut self.state {
            ARAFCState::Idle(core) => {
                if core.inner_buf_offset < core.inner_buf_len {
                    // we have data in the inner buffer, don't even need
                    // to spawn a blocking task
                    let read_len =
                        cmp::min(buf.remaining(), core.inner_buf_len - core.inner_buf_offset);

                    buf.put_slice(&core.inner_buf[core.inner_buf_offset..][..read_len]);
                    core.inner_buf_offset += read_len;
                    trace!(inner_buf_offset = %core.inner_buf_offset, inner_buf_len = %core.inner_buf_len, "read from inner buffer");

                    return Poll::Ready(Ok(()));
                }

                // this is just used to shadow core
                #[allow(unused_variables, clippy::let_unit_value)]
                let core = ();

                let (file_offset, file, mut inner_buf) = {
                    let core = match std::mem::take(&mut self.state) {
                        ARAFCState::Idle(core) => core,
                        _ => unreachable!(),
                    };
                    (core.file_offset, core.file, core.inner_buf)
                };

                let fut = Box::pin(tokio::task::spawn_blocking(move || {
                    let read_bytes = file.read_at(file_offset, &mut inner_buf)?;
                    trace!(%read_bytes, "read from file");
                    Ok(ARAFCCore {
                        file_offset: file_offset + read_bytes as u64,
                        file,
                        inner_buf,
                        inner_buf_len: read_bytes,
                        inner_buf_offset: 0,
                    })
                }));
                self.state = ARAFCState::Reading { fut };
                self.poll_read(cx, buf)
            }
            ARAFCState::Reading { fut } => {
                let core =
                    futures_util::ready!(fut.as_mut().poll(cx).map_err(io::Error::other)??);
                let is_eof = core.inner_buf_len == 0;
                self.state = ARAFCState::Idle(core);

                if is_eof {
                    // we're at EOF
                    return Poll::Ready(Ok(()));
                }
                self.poll_read(cx, buf)
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
