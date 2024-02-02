//! A library for reading zip files synchronously using std I/O traits,
//! built on top of [rc-zip](https://crates.io/crates/rc-zip).

#![warn(missing_docs)]

macro_rules! transition {
    ($state: expr => ($pattern: pat) $body: expr) => {
        $state = if let $pattern = std::mem::take(&mut $state) {
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
pub use read_zip::{HasCursor, ReadZip, ReadZipWithSize, SyncArchive, SyncStoredEntry};
