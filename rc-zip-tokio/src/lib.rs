//! A library for reading zip files asynchronously using tokio I/O traits,
//! based on top of [rc-zip](https://crates.io/crates/rc-zip).

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

mod decoder;
mod entry_reader;
mod read_zip;

// re-exports
pub use rc_zip;
pub use read_zip::{
    AsyncArchive, AsyncReadZip, AsyncReadZipWithSize, AsyncStoredEntry, HasAsyncCursor,
};
