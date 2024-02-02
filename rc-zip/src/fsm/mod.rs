//! Parsers are just part of the puzzle when it comes to zip files: finding the
//! central directory is non-trivial and involves seeking around the input:
//! [ArchiveFsm] provides a state machine to handle this.
//!
//! Similarly, reading an entry involves reading the local header, then the
//! data (while calculating the CRC32), then the data descriptor, and then
//! checking whether the uncompressed size and CRC32 match the values in the
//! central directory.

macro_rules! transition {
    ($state: expr => ($pattern: pat) $body: expr) => {
        $state = if let $pattern = std::mem::take(&mut $state) {
            $body
        } else {
            unreachable!()
        };
    };
}

mod archive;
pub use archive::ArchiveFsm;

mod entry;
pub use entry::EntryFsm;

/// Indicates whether or not the state machine has completed its work
pub enum FsmResult<T> {
    /// Indicates that the state machine still has work to do, and
    /// needs either data or a call to process
    Continue,
    /// Indicates that the state machine has completed its work, and
    /// the result is the value provided
    Done(T),
}
