//! Prelude for rc-zip

#[cfg(feature = "sync")]
pub use crate::reader::sync::{ReadZip, ReadZipWithSize};
#[cfg(feature = "sync")]
pub use positioned_io::ReadAt;
