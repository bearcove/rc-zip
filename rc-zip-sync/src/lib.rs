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
pub use entry_reader::EntryReader;
pub use read_zip::{HasCursor, ReadZip, ReadZipWithSize, SyncArchive, SyncStoredEntry};
