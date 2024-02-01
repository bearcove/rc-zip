mod decoder;
use decoder::*;

mod entry_reader;
use entry_reader::*;

mod read_zip;

// re-exports
pub use read_zip::{ReadZip, ReadZipWithSize};
