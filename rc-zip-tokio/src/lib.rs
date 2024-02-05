//! A library for reading zip files asynchronously using tokio I/O traits,
//! based on top of [rc-zip](https://crates.io/crates/rc-zip).
//!
//! See also:
//!
//!   * [rc-zip-sync](https://crates.io/crates/rc-zip-sync) for using std I/O traits

#![warn(missing_docs)]

mod async_read_zip;
mod entry_reader;

// re-exports
pub use async_read_zip::{
    AsyncArchive, AsyncEntry, HasAsyncCursor, ReadZipAsync, ReadZipWithSizeAsync,
};
pub use rc_zip;
