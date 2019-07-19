//! Prelude for rc-zip

pub use positioned_io::ReadAt;

// Re-export archive traits
pub use crate::reader::{ReadZip, ReadZipWithSize};
