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
pub use archive::ArchiveReader;

mod entry;

/// Indicates whether or not the state machine has completed its work
pub enum FsmResult<T> {
    /// Indicates that the state machine still has work to do, and
    /// needs either data or a call to process
    Continue,
    /// Indicates that the state machine has completed its work, and
    /// the result is the value provided
    Done(T),
}
