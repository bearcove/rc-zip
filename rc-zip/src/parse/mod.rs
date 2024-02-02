//! Contain winnow parsers for most elements that make up a ZIP file, like the
//! end-of-central-directory record, local file headers, and central directory
//! headers.
//!
//! All parsers here are based off of the PKWARE appnote.txt, which you can find
//! in the source repository.

pub use crate::encoding::Encoding;

mod archive;
pub use archive::*;

mod extra_field;
pub use extra_field::*;

mod mode;
pub use mode::*;

mod version;
pub use version::*;

mod date_time;
pub use date_time::*;

mod directory_header;
pub use directory_header::*;

mod eocd;
pub use eocd::*;

mod local;
pub use local::*;

mod raw;
pub use raw::*;
