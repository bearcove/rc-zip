use super::error::*;
use hex_fmt::HexFmt;
use positioned_io::ReadAt;
use std::fmt;

#[derive(Debug)]
pub(crate) struct EndOfCentralDirectoryRecord {
    /// number of this disk
    pub(crate) disk_nbr: u16,
    /// number of the disk with the start of the central directory
    pub(crate) dir_disk_nbr: u16,
    /// total number of entries in the central directory on this disk
    pub(crate) dir_records_this_disk: u16,
    /// total number of entries in the central directory
    pub(crate) directory_records: u16,
    // size of the central directory
    pub(crate) directory_size: u32,
    /// offset of start of central directory with respect to the starting diskn umber
    pub(crate) directory_offset: u32,
    /// .ZIP file comment
    pub(crate) comment: ZipString,
}

#[derive(Debug)]
pub(crate) struct EndOfCentralDirectory64Record {
    /// size of zip64 end of central directory record
    pub(crate) record_size: u64,
    /// version made by
    pub(crate) version_made_by: u16,
    /// version needed to extract
    pub(crate) version_needed: u16,
    /// number of this disk
    pub(crate) disk_nbr: u32,
    /// number of the disk with the start of the central directory
    pub(crate) dir_disk_nbr: u32,
    // total number of entries in the central directory on this disk
    pub(crate) dir_records_this_disk: u64,
    // total number of entries in the central directory
    pub(crate) directory_records: u64,
    // size of the central directory
    pub(crate) directory_size: u64,
    // offset of the start of central directory with respect to the
    // starting disk number
    pub(crate) directory_offset: u64,
}

#[derive(Debug)]
pub(crate) struct EndOfCentralDirectory {
    pub(crate) directory_end: EndOfCentralDirectoryRecord,
    pub(crate) directory64_end: Option<EndOfCentralDirectory64Record>,
    pub(crate) start_skip_len: usize,
}

pub struct ZipString(pub Vec<u8>);

impl fmt::Debug for ZipString {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match std::str::from_utf8(&self.0) {
            Ok(s) => write!(f, "{:?}", s),
            Err(_) => write!(f, "[non-utf8 string: {:x}]", HexFmt(&self.0)),
        }
    }
}

pub struct ZipReader<'a, R>
where
    R: ReadAt,
{
    pub(crate) reader: &'a R,
    pub(crate) size: usize,
}

impl<'a, R> ZipReader<'a, R>
where
    R: ReadAt,
{
    pub fn new(reader: &'a R, size: usize) -> Result<Self, Error> {
        let directory_end = super::parser::read_end_of_central_directory(reader, size)?;
        println!("directory_end = {:#?}", directory_end);

        Ok(Self { reader, size })
    }

    fn entries(&self) -> &[ZipEntry<'a>] {
        unimplemented!()
    }
}

pub struct ZipEntry<'a> {
    name: &'a str,
}

impl<'a> ZipEntry<'a> {
    fn name() -> &'a str {
        unimplemented!()
    }
}
