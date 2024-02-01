mod decoder;
mod entry_reader;
mod read_zip;

// re-exports
pub use entry_reader::EntryReader;
pub use read_zip::{HasCursor, ReadZip, ReadZipWithSize, SyncArchive, SyncStoredEntry};
