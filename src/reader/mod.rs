mod buffer;
mod macros;

mod archive_reader;
use oval::Buffer;

pub use self::archive_reader::{ArchiveReader, ArchiveReaderResult};

#[cfg(feature = "sync")]
pub mod sync;

#[cfg(feature = "tokio")]
pub mod tokio;

/// Only allows reading a fixed number of bytes from a [oval::Buffer],
/// used for reading the raw (compressed) data for a single zip file entry.
/// It also allows moving out the inner buffer afterwards.
pub(crate) struct RawEntryReader {
    remaining: u64,
    inner: Buffer,
}

impl RawEntryReader {
    pub(crate) fn new(inner: Buffer, entry_size: u64) -> Self {
        Self {
            inner,
            remaining: entry_size,
        }
    }

    pub(crate) fn into_inner(self) -> Buffer {
        self.inner
    }

    pub(crate) fn get_mut(&mut self) -> &mut Buffer {
        &mut self.inner
    }
}
