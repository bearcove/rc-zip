mod buffer;
mod macros;

mod archive_reader;
pub use self::archive_reader::{ArchiveReader, ArchiveReaderResult};

#[cfg(feature = "sync")]
pub mod sync;
