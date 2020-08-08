pub use crate::encoding::Encoding;

#[macro_use]
pub(crate) mod parse;
pub(crate) use fields;

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
