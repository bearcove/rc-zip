//! A library for reading zip files synchronously using std I/O traits,
//! built on top of [rc-zip](https://crates.io/crates/rc-zip).
//!
//! See also:
//!
//!   * [rc-zip-tokio](https://crates.io/crates/rc-zip-tokio) for using tokio I/O traits

#![warn(missing_docs)]

mod entry_reader;
mod read_zip;

// re-exports
pub use rc_zip;
pub use read_zip::{HasCursor, ReadZip, ReadZipWithSize, SyncArchive, SyncStoredEntry};
