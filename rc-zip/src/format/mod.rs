//! Contain winnow parsers for most elements that make up a ZIP file, like
//! the end-of-central-directory record, local file headers, and central
//! directory headers.
//!
//! Everything in there is based off of the appnote, which you can find in the
//! source repository.

pub use crate::encoding::Encoding;

mod archive;
mod extra_field;
mod mode;
mod version;
pub use self::{archive::*, extra_field::*, mode::*, version::*};

mod date_time;
mod directory_header;
mod eocd;
mod local;
mod raw;
pub use self::{date_time::*, directory_header::*, eocd::*, local::*, raw::*};

use chrono::{offset::Utc, DateTime};
