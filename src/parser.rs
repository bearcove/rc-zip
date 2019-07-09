use super::error::*;
use hex_fmt::HexFmt;
use positioned_io::ReadAt;
use std::fmt;

use nom::{
    bytes::complete::tag,
    combinator::map,
    error::ParseError,
    multi::length_data,
    number::complete::{le_u16, le_u32, le_u64},
    sequence::{preceded, tuple},
    IResult,
};

// Reference code for zip handling:
// https://github.com/itchio/arkive/blob/master/zip/reader.go

// 4.3.15 Zip64 end of central directory locator
fn end_of_central_directory64_locator<'a, E: ParseError<&'a [u8]>>(
    i: &'a [u8],
) -> IResult<&'a [u8], EndOfCentralDirectory64Locator, E> {
    preceded(
        tag("PK\x06\x07"),
        map(tuple((le_u32, le_u64, le_u32)), |t| {
            EndOfCentralDirectory64Locator {
                dir_disk_number: t.0,
                directory_offset: t.1,
                total_disks: t.2,
            }
        }),
    )(i)
}

// 4.3.14  Zip64 end of central directory record
fn end_of_central_directory64_record<'a, E: ParseError<&'a [u8]>>(
    i: &'a [u8],
) -> IResult<&'a [u8], EndOfCentralDirectory64Record, E> {
    preceded(
        tag("PK\x06\x06"),
        map(
            tuple((
                le_u64, le_u16, le_u16, le_u32, le_u32, le_u64, le_u64, le_u64, le_u64,
            )),
            |t| EndOfCentralDirectory64Record {
                record_size: t.0,
                version_made_by: t.1,
                version_needed: t.2,
                disk_nbr: t.3,
                dir_disk_nbr: t.4,
                dir_records_this_disk: t.5,
                directory_records: t.6,
                directory_size: t.7,
                directory_offset: t.8,
            },
        ),
    )(i)
}

fn find_signature_in_block(b: &[u8]) -> Option<(usize, EndOfCentralDirectoryRecord)> {
    for i in (0..(b.len() - EndOfCentralDirectoryRecord::LENGTH + 1)).rev() {
        let slice = &b[i..];

        if let Ok((_, directory)) = EndOfCentralDirectoryRecord::parse::<DecodingError>(slice) {
            return Some((i, directory));
        }
    }
    None
}

fn find_end_of_central_directory_record<R: ReadAt>(
    reader: &R,
    size: usize,
) -> Result<Option<(usize, EndOfCentralDirectoryRecord)>, Error> {
    let ranges: [usize; 2] = [1024, 65 * 1024];
    for &b_len in &ranges {
        let b_len = std::cmp::min(b_len, size);
        let mut buf = vec![0u8; b_len];
        reader.read_exact_at((size - b_len) as u64, &mut buf)?;

        if let Some((offset, directory)) = find_signature_in_block(&buf[..]) {
            let offset = size - b_len + offset;
            return Ok(Some((offset, directory)));
        }
    }
    Ok(None)
}

fn find_end_of_central_directory64_record<R: ReadAt>(
    reader: &R,
    directory_end_offset: usize,
) -> Result<Option<(usize, EndOfCentralDirectory64Record)>, Error> {
    if directory_end_offset < EndOfCentralDirectory64Locator::LENGTH {
        // no need to look for a header outside the file
        return Ok(None);
    }

    let loc_offset = directory_end_offset - EndOfCentralDirectory64Locator::LENGTH;

    let mut locbuf = vec![0u8; EndOfCentralDirectory64Locator::LENGTH];
    reader.read_exact_at(loc_offset as u64, &mut locbuf)?;
    let locres = end_of_central_directory64_locator::<DecodingError>(&locbuf[..]);

    if let Ok((_, locator)) = locres {
        println!("locator: {:#?}", locator);
        if locator.dir_disk_number != 0 {
            // the file is not a valid zip64 file
            return Ok(None);
        }

        if locator.total_disks != 1 {
            // the file is not a valid zip64 file
            return Ok(None);
        }

        let offset = locator.directory_offset as usize;
        println!("reading EOD64R at: {}", offset);
        let mut recbuf = vec![0u8; EndOfCentralDirectory64Record::LENGTH];
        reader.read_exact_at(offset as u64, &mut recbuf)?;
        let recres = end_of_central_directory64_record::<DecodingError>(&recbuf[..]);

        if let Ok((_, record)) = recres {
            return Ok(Some((offset, record)));
        } else {
            println!("recres = {:#?}", recres);
        }
    }

    Ok(None)
}

pub(crate) fn read_end_of_central_directory<R: ReadAt>(
    reader: &R,
    size: usize,
) -> Result<EndOfCentralDirectory, Error> {
    let (d_offset, d) = find_end_of_central_directory_record(reader, size)?
        .ok_or(FormatError::DirectoryEndSignatureNotFound)?;

    // These values mean that the file can be a zip64 file
    //
    // However, on macOS, some .zip files have a zip64 directory
    // but doesn't have these values, cf. https://github.com/itchio/butler/issues/141
    let probably_zip64 =
        d.directory_records == 0xffff || d.directory_size == 0xffff || d.directory_offset == 0xffff;

    let mut d64_info: Option<(usize, EndOfCentralDirectory64Record)> = None;

    let res64 = find_end_of_central_directory64_record(reader, d_offset);
    match res64 {
        Ok(Some(found_d64_info)) => {
            d64_info = Some(found_d64_info);
        }
        Ok(None) => { /* not a zip64 file, that's ok! */ }
        Err(e) => {
            if probably_zip64 {
                return Err(e);
            }
        }
    }

    let computed_directory_offset = match d64_info.as_ref() {
        // cf. https://users.cs.jmu.edu/buchhofp/forensics/formats/pkzip.html
        // `directorySize` does not include
        //  - Zip64 end of central directory record
        //  - Zip64 end of central directory locator
        // and we don't want to be a few bytes off, now do we.
        Some((d64_offset, d64)) => *d64_offset - d64.directory_size as usize,
        None => d_offset - d.directory_size as usize,
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
    // <-start_skip_len->                    <------directory_size----->
    // [    Padding     ][ Data 1 ][ Data 2 ][    Central directory    ][ ??? ]
    // ^                 ^                   ^                         ^
    // 0                 start_skip_len      computed_directory_offset directory_end_offset
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

    let mut eocd = EndOfCentralDirectory {
        dir: d,
        dir64: d64_info.map(|(_offset, record)| record),
        start_skip_len: 0,
    };

    // did we find a valid offset?
    if (0..size).contains(&computed_directory_offset) {
        // that's different from the recorded one?
        if computed_directory_offset != eocd.directory_offset() {
            // then assume `start_skip_len` padding
            eocd.start_skip_len = computed_directory_offset - eocd.directory_offset();
            eocd.set_directory_offset(computed_directory_offset);
        }
    }

    // make sure directory_offset points to somewhere in our file
    if !(0..size).contains(&eocd.directory_offset()) {
        return Err(FormatError::DirectoryOffsetPointsOutsideFile.into());
    }

    Ok(eocd)
}

#[derive(Debug)]
/// 4.3.16  End of central directory record:
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
    /// offset of start of central directory with respect to the starting disk number
    pub(crate) directory_offset: u32,
    /// .ZIP file comment
    pub(crate) comment: ZipString,
}

impl EndOfCentralDirectoryRecord {
    /// does not include comment size & comment data
    pub(crate) const LENGTH: usize = 20;

    fn parse<'a, E: ParseError<&'a [u8]>>(i: &'a [u8]) -> IResult<&'a [u8], Self, E> {
        preceded(
            tag("PK\x05\x06"),
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
                |t| Self {
                    disk_nbr: t.0,
                    dir_disk_nbr: t.1,
                    dir_records_this_disk: t.2,
                    directory_records: t.3,
                    directory_size: t.4,
                    directory_offset: t.5,
                    comment: ZipString(t.6.into()),
                },
            ),
        )(i)
    }
}

#[derive(Debug)]
pub(crate) struct EndOfCentralDirectory64Locator {
    /// number of the disk with the start of the zip64 end of central directory
    pub(crate) dir_disk_number: u32,
    /// relative offset of the zip64 end of central directory record
    pub(crate) directory_offset: u64,
    /// total number of disks
    pub(crate) total_disks: u32,
}

impl EndOfCentralDirectory64Locator {
    pub(crate) const LENGTH: usize = 20;
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

impl EndOfCentralDirectory64Record {
    pub(crate) const LENGTH: usize = 56;
}

#[derive(Debug)]
pub(crate) struct EndOfCentralDirectory {
    pub(crate) dir: EndOfCentralDirectoryRecord,
    pub(crate) dir64: Option<EndOfCentralDirectory64Record>,
    pub(crate) start_skip_len: usize,
}

impl EndOfCentralDirectory {
    pub(crate) fn directory_offset(&self) -> usize {
        match self.dir64.as_ref() {
            Some(d64) => d64.directory_offset as usize,
            None => self.dir.directory_offset as usize,
        }
    }

    pub(crate) fn set_directory_offset(&mut self, offset: usize) {
        match self.dir64.as_mut() {
            Some(d64) => d64.directory_offset = offset as u64,
            None => self.dir.directory_offset = offset as u32,
        };
    }
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
