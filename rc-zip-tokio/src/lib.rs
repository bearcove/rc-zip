//! A library for reading zip files asynchronously using tokio I/O traits,
//! based on top of [rc-zip](https://crates.io/crates/rc-zip).
//!
//! See also:
//!
//!   * [rc-zip-sync](https://crates.io/crates/rc-zip-sync) for using std I/O traits

#![warn(missing_docs)]

mod entry_reader;
mod read_zip;

mod streaming_entry_reader;
pub use streaming_entry_reader::StreamingEntryReader;

// re-exports
pub use rc_zip;
pub use read_zip::{
    ArchiveHandle, EntryHandle, HasCursor, ReadZip, ReadZipEntriesStreaming, ReadZipWithSize,
};
