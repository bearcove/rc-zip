//! A library for reading zip files asynchronously using tokio I/O traits,
//! based on top of [rc-zip](https://crates.io/crates/rc-zip).
//!
//! See also:
//!
//!   * [rc-zip-sync](https://crates.io/crates/rc-zip-sync) for using std I/O traits

#![warn(missing_docs)]

macro_rules! transition_async {
    ($state: expr => ($pattern: pat) $body: expr) => {
        *$state.as_mut() = if let $pattern = std::mem::take($state.as_mut().get_mut()) {
            $body
        } else {
            unreachable!()
        };
    };
}

mod async_read_zip;
mod decoder;
mod entry_reader;

// re-exports
pub use async_read_zip::{
    AsyncArchive, AsyncReadZip, AsyncReadZipWithSize, AsyncStoredEntry, HasAsyncCursor,
};
pub use rc_zip;
