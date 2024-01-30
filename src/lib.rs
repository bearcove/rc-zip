//! # rc-zip
//!
//! rc-zip is a zip archive library with a focus on compatibility and correctness.
//!
//! ### Reading
//!
//! [ArchiveReader](reader::ArchiveReader) is your first stop. It
//! ensures we are dealing with a valid zip archive, and reads the central
//! directory. It does not perform I/O itself, but rather, it is a state machine
//! that asks for reads at specific offsets.
//!
//! An [Archive] contains a full list of [entries](StoredEntry),
//! which you can then extract.
//!
//! ### Writing
//!
//! Writing archives is not implemented yet.
//!

mod encoding;
mod error;
mod format;
pub mod prelude;
pub mod reader;

pub use self::{error::*, format::*};

#[cfg(test)]
mod tests;
