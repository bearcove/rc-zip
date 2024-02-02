#![warn(missing_docs)]

//! rc-zip is a [sans-io](https://sans-io.readthedocs.io/how-to-sans-io.html) library for reading zip files.
//!
//! It's made up of a bunch of types representing the various parts of a zip
//! file, winnow parsers that can turn byte buffers into those types, and
//! state machines that can use those parsers to read zip files from a stream.
//!
//! [rc-zip-sync](https://crates.io/crates/rc-zip-sync) and
//! [rc-zip-tokio](https://crates.io/crates/rc-zip-tokio) build on top of this
//! to provide a higher-level API for reading zip files, from sync and async
//! code respectively.

pub mod encoding;
pub mod error;
pub mod fsm;
pub mod parse;
