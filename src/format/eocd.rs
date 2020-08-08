use crate::{error::*, format::*};
use log::*;
use nom::{
    bytes::streaming::tag,
    combinator::map,
    multi::length_data,
    number::streaming::{le_u16, le_u32, le_u64},
    sequence::{preceded, tuple},
};

/// 4.3.16  End of central directory record:
#[derive(Debug)]
pub struct EndOfCentralDirectoryRecord {
    /// number of this disk
    pub disk_nbr: u16,
    /// number of the disk with the start of the central directory
    pub dir_disk_nbr: u16,
    /// total number of entries in the central directory on this disk
    pub dir_records_this_disk: u16,
    /// total number of entries in the central directory
    pub directory_records: u16,
    // size of the central directory
    pub directory_size: u32,
    /// offset of start of central directory with respect to the starting disk number
    pub directory_offset: u32,
    /// .ZIP file comment
    pub comment: ZipString,
}

impl EndOfCentralDirectoryRecord {
    /// Does not include comment size & comment data
    const MIN_LENGTH: usize = 20;
    const SIGNATURE: &'static str = "PK\x05\x06";

    pub fn find_in_block(b: &[u8]) -> Option<Located<Self>> {
        for i in (0..(b.len() - Self::MIN_LENGTH + 1)).rev() {
            let slice = &b[i..];

            if let Ok((_, directory)) = Self::parse(slice) {
                return Some(Located {
                    offset: i as u64,
                    inner: directory,
                });
            }
        }
        None
    }

    pub fn parse<'a>(i: &'a [u8]) -> parse::Result<'a, Self> {
        preceded(
            tag(Self::SIGNATURE),
            map(
                tuple((
                    le_u16,
                    le_u16,
                    le_u16,
                    le_u16,
                    le_u32,
                    le_u32,
                    length_data(le_u16),
                )),
                |(
                    disk_nbr,
                    dir_disk_nbr,
                    dir_records_this_disk,
                    directory_records,
                    directory_size,
                    directory_offset,
                    comment,
                )| Self {
                    disk_nbr,
                    dir_disk_nbr,
                    dir_records_this_disk,
                    directory_records,
                    directory_size,
                    directory_offset,
                    comment: comment.into(),
                },
            ),
        )(i)
    }
}

#[derive(Debug)]
/// 4.3.15 Zip64 end of central directory locator
pub struct EndOfCentralDirectory64Locator {
    /// number of the disk with the start of the zip64 end of central directory
    pub dir_disk_number: u32,
    /// relative offset of the zip64 end of central directory record
    pub directory_offset: u64,
    /// total number of disks
    pub total_disks: u32,
}

impl EndOfCentralDirectory64Locator {
    pub const LENGTH: usize = 20;
    const SIGNATURE: &'static str = "PK\x06\x07";

    pub fn parse<'a>(i: &'a [u8]) -> parse::Result<'a, Self> {
        preceded(
            tag(Self::SIGNATURE),
            fields!(Self {
                dir_disk_number: le_u32,
                directory_offset: le_u64,
                total_disks: le_u32,
            }),
        )(i)
    }
}

#[derive(Debug)]
/// 4.3.14  Zip64 end of central directory record
pub struct EndOfCentralDirectory64Record {
    /// size of zip64 end of central directory record
    pub record_size: u64,
    /// version made by
    pub creator_version: u16,
    /// version needed to extract
    pub reader_version: u16,
    /// number of this disk
    pub disk_nbr: u32,
    /// number of the disk with the start of the central directory
    pub dir_disk_nbr: u32,
    // total number of entries in the central directory on this disk
    pub dir_records_this_disk: u64,
    // total number of entries in the central directory
    pub directory_records: u64,
    // size of the central directory
    pub directory_size: u64,
    // offset of the start of central directory with respect to the
    // starting disk number
    pub directory_offset: u64,
}

impl EndOfCentralDirectory64Record {
    const SIGNATURE: &'static str = "PK\x06\x06";

    pub fn parse<'a>(i: &'a [u8]) -> parse::Result<'a, Self> {
        preceded(
            tag(Self::SIGNATURE),
            fields!(Self {
                record_size: le_u64,
                creator_version: le_u16,
                reader_version: le_u16,
                disk_nbr: le_u32,
                dir_disk_nbr: le_u32,
                dir_records_this_disk: le_u64,
                directory_records: le_u64,
                directory_size: le_u64,
                directory_offset: le_u64,
            }),
        )(i)
    }
}

#[derive(Debug)]
pub struct Located<T> {
    pub offset: u64,
    pub inner: T,
}

impl<T> std::ops::Deref for Located<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> std::ops::DerefMut for Located<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[derive(Debug)]
/// Coalesces zip and zip64 "end of central directory" record info
pub struct EndOfCentralDirectory {
    pub dir: Located<EndOfCentralDirectoryRecord>,
    pub dir64: Option<Located<EndOfCentralDirectory64Record>>,
    pub global_offset: i64,
}

impl EndOfCentralDirectory {
    pub fn new(
        size: u64,
        dir: Located<EndOfCentralDirectoryRecord>,
        dir64: Option<Located<EndOfCentralDirectory64Record>>,
    ) -> Result<Self, Error> {
        let mut res = Self {
            dir,
            dir64,
            global_offset: 0,
        };

        //
        // Pure .zip files look like this:
        // ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
        //                     <------directory_size----->
        // [ Data 1 ][ Data 2 ][    Central directory    ][ ??? ]
        // ^                   ^                          ^
        // 0                   directory_offset           directory_end_offset
        // ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
        //
        // But there exist some valid zip archives with padding at the beginning, like so:
        // ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
        // <--global_offset->                    <------directory_size----->
        // [    Padding     ][ Data 1 ][ Data 2 ][    Central directory    ][ ??? ]
        // ^                 ^                   ^                         ^
        // 0                 global_offset       computed_directory_offset directory_end_offset
        // ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
        //
        // (e.g. https://www.icculus.org/mojosetup/ installers are ELF binaries with a .zip file appended)
        //
        // `directory_end_offfset` is found by scanning the file (so it accounts for padding), but
        // `directory_offset` is found by reading a data structure (so it does not account for padding).
        // If we just trusted `directory_offset`, we'd be reading the central directory at the wrong place:
        // ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
        //                                       <------directory_size----->
        // [    Padding     ][ Data 1 ][ Data 2 ][    Central directory    ][ ??? ]
        // ^                   ^                                           ^
        // 0                   directory_offset - woops!                   directory_end_offset
        // ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

        let computed_directory_offset = res.located_directory_offset() - res.directory_size();

        // did we find a valid offset?
        if (0..size).contains(&computed_directory_offset) {
            // that's different from the recorded one?
            if computed_directory_offset != res.directory_offset() {
                // then assume the whole file is offset
                res.global_offset =
                    computed_directory_offset as i64 - res.directory_offset() as i64;
                res.set_directory_offset(computed_directory_offset);
            }
        }

        // make sure directory_offset points to somewhere in our file
        debug!(
            "directory offset = {}, valid range = 0..{}",
            res.directory_offset(),
            size
        );
        if !(0..size).contains(&res.directory_offset()) {
            return Err(FormatError::DirectoryOffsetPointsOutsideFile.into());
        }

        Ok(res)
    }

    pub fn located_directory_offset(&self) -> u64 {
        match self.dir64.as_ref() {
            Some(d64) => d64.offset,
            None => self.dir.offset,
        }
    }

    pub fn directory_offset(&self) -> u64 {
        match self.dir64.as_ref() {
            Some(d64) => d64.directory_offset,
            None => self.dir.directory_offset as u64,
        }
    }

    pub fn directory_size(&self) -> u64 {
        match self.dir64.as_ref() {
            Some(d64) => d64.directory_size,
            None => self.dir.directory_size as u64,
        }
    }

    pub fn set_directory_offset(&mut self, offset: u64) {
        match self.dir64.as_mut() {
            Some(d64) => d64.directory_offset = offset,
            None => self.dir.directory_offset = offset as u32,
        };
    }

    pub fn directory_records(&self) -> u64 {
        match self.dir64.as_ref() {
            Some(d64) => d64.directory_records,
            None => self.dir.directory_records as u64,
        }
    }

    pub fn comment(&self) -> &ZipString {
        &self.dir.comment
    }
}
