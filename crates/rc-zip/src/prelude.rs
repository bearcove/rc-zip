//! Prelude for rc-zip

#[cfg(feature = "sync")]
pub use crate::reader::sync::{ReadZip, ReadZipWithSize};
