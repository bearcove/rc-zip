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
pub use entry_reader::AsyncEntryReader;
pub use read_zip::{
    AsyncArchive, AsyncReadZip, AsyncReadZipWithSize, AsyncStoredEntry, HasAsyncCursor,
};
