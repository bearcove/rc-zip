mod buffer;

#[macro_use]
mod macros;

mod archive_reader;
pub use self::archive_reader::{ArchiveReader, ArchiveReaderResult};

#[cfg(feature = "async-ara")]
pub mod async_ara;

#[cfg(feature = "sync")]
pub mod sync;
