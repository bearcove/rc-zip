#![allow(unused)]

use super::{
    encoding::{self, Encoding},
    error::*,
    types::*,
};
use log::*;
#[macro_use]
mod nom_macros;

use hex_fmt::HexFmt;
use positioned_io::{Cursor, ReadAt};
use std::fmt;
use std::io::Read;

use nom::{
    bytes::complete::{tag, take},
    combinator::map,
    error::ParseError,
    multi::length_data,
    number::complete::{le_u16, le_u32, le_u64},
    sequence::{preceded, tuple},
    IResult, Offset,
};

// Reference code for zip handling:
// https://github.com/itchio/arkive/blob/master/zip/reader.go

#[repr(u16)]
enum ExtraHeaderID {
    /// Zip64 extended information
    Zip64 = 0x0001,
    /// NTFS
    NTFS = 0x000a,
    /// UNIX
    Unix = 0x000d,
    // Extended timestamp
    ExtTime = 0x5455,
    /// Info-ZIP Unix extension
    InfoZipUnix = 0x5855,
}

#[derive(Debug)]
/// 4.3.7 Local file header
struct LocalFileHeaderRecord {
    /// version needed to extract
    reader_version: u16,
    /// general purpose bit flag
    flags: u16,
    /// compression method
    method: u16,
    /// last mod file time
    modified_time: u16,
    /// last mod file date
    modified_date: u16,
    /// crc-32
    crc32: u32,
    /// compressed size
    compressed_size: u32,
    /// uncompressed size
    uncompressed_size: u32,
    // file name
    name: ZipString,
    // extra field
    extra: ZipBytes,
}

impl LocalFileHeaderRecord {
    /// Does not include filename size & data, extra size & data
    const LENGTH: usize = 30;
    const SIGNATURE: &'static str = "PK\x03\x04";
}

// 4.3.12 Central directory structure: File header
#[derive(Debug)]
struct FileHeaderRecord {
    // version made by
    creator_version: u16,
    // version needed to extract
    reader_version: u16,
    // general purpose bit flag
    flags: u16,
    // compression method
    method: u16,
    // last mod file time
    modified_time: u16,
    // last mod file date
    modified_date: u16,
    // crc32
    crc32: u32,
    // compressed size
    compressed_size: u32,
    // uncompressed size
    uncompressed_size: u32,
    // disk number start
    disk_nbr_start: u16,
    // internal file attributes
    internal_attrs: u16,
    // external file attributes
    external_attrs: u32,
    // relative offset of local header
    header_offset: u32,

    // name
    name: ZipString,
    // extra
    extra: ZipBytes,
    // comment
    comment: ZipString,
}

impl FileHeaderRecord {
    const SIGNATURE: &'static str = "PK\x01\x02";

    fn parse<'a, E: ParseError<&'a [u8]>>(i: &'a [u8]) -> IResult<&'a [u8], Self, E> {
        preceded(
            tag(Self::SIGNATURE),
            fields!({
                creator_version: le_u16,
                reader_version: le_u16,
                flags: le_u16,
                method: le_u16,
                modified_time: le_u16,
                modified_date: le_u16,
                crc32: le_u32,
                compressed_size: le_u32,
                uncompressed_size: le_u32,
                name_len: le_u16,
                extra_len: le_u16,
                comment_len: le_u16,
                disk_nbr_start: le_u16,
                internal_attrs: le_u16,
                external_attrs: le_u32,
                header_offset: le_u32,
            } chain {
                fields!({
                    name: zip_string(name_len),
                    extra: zip_bytes(extra_len),
                    comment: zip_string(comment_len),
                } map Self {
                    creator_version,
                    reader_version,
                    flags,
                    method,
                    modified_time,
                    modified_date,
                    crc32,
                    compressed_size,
                    uncompressed_size,
                    disk_nbr_start,
                    internal_attrs,
                    external_attrs,
                    header_offset,
                    name: name,
                    extra: extra,
                    comment: comment,
                })
            }),
        )(i)
    }
}

impl FileHeaderRecord {
    fn is_non_utf8(&self) -> bool {
        let (valid1, require1) = encoding::detect_utf8(&self.name.0[..]);
        let (valid2, require2) = encoding::detect_utf8(&self.comment.0[..]);
        if !valid1 || !valid2 {
            // definitely not utf-8
            return true;
        }

        if !require1 && !require2 {
            // name and comment only use single-byte runes that overlap with UTF-8
            return false;
        }

        // Might be UTF-8, might be some other encoding; preserve existing flag.
        // Some ZIP writers use UTF-8 encoding without setting the UTF-8 flag.
        // Since it is impossible to always distinguish valid UTF-8 from some
        // other encoding (e.g., GBK or Shift-JIS), we trust the flag.
        self.flags & 0x800 == 0
    }

    fn as_file_header(self, encoding: Encoding) -> Result<FileHeader, encoding::DecodingError> {
        let mut comment: Option<String> = None;
        if let Some(comment_field) = self.comment.as_option() {
            comment = Some(encoding.decode(&comment_field.0)?);
        }

        Ok(FileHeader {
            name: encoding.decode(&self.name.0)?,
            comment,
            creator_version: self.creator_version,
            reader_version: self.reader_version,
            flags: self.flags,
            modified: zero_datetime(),

            crc32: self.crc32,
            compressed_size: self.compressed_size as u64,
            uncompressed_size: self.uncompressed_size as u64,

            extra: self.extra.as_option().map(|x| x.0),
            external_attrs: self.external_attrs,
        })
    }
}

#[derive(Debug)]
/// 4.3.16  End of central directory record:
struct EndOfCentralDirectoryRecord {
    /// number of this disk
    disk_nbr: u16,
    /// number of the disk with the start of the central directory
    dir_disk_nbr: u16,
    /// total number of entries in the central directory on this disk
    dir_records_this_disk: u16,
    /// total number of entries in the central directory
    directory_records: u16,
    // size of the central directory
    directory_size: u32,
    /// offset of start of central directory with respect to the starting disk number
    directory_offset: u32,
    /// .ZIP file comment
    comment: ZipString,
}

impl EndOfCentralDirectoryRecord {
    /// does not include comment size & comment data
    const LENGTH: usize = 20;
    const SIGNATURE: &'static str = "PK\x05\x06";

    fn read<R: ReadAt>(reader: R, size: u64) -> Result<Option<(u64, Self)>, Error> {
        let ranges: [u64; 2] = [1024, 65 * 1024];
        for &b_len in &ranges {
            let b_len = std::cmp::min(b_len, size);
            let mut buf = vec![0u8; b_len as usize];
            reader.read_exact_at(size - b_len, &mut buf)?;

            if let Some((offset, directory)) = Self::find_in_block(&buf[..]) {
                let offset = size - b_len + offset as u64;
                return Ok(Some((offset, directory)));
            }
        }
        Ok(None)
    }

    fn find_in_block(b: &[u8]) -> Option<(usize, Self)> {
        for i in (0..(b.len() - Self::LENGTH + 1)).rev() {
            let slice = &b[i..];

            if let Ok((_, directory)) = Self::parse::<ZipParseError>(slice) {
                return Some((i, directory));
            }
        }
        None
    }

    fn parse<'a, E: ParseError<&'a [u8]>>(i: &'a [u8]) -> IResult<&'a [u8], Self, E> {
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
struct EndOfCentralDirectory64Locator {
    /// number of the disk with the start of the zip64 end of central directory
    dir_disk_number: u32,
    /// relative offset of the zip64 end of central directory record
    directory_offset: u64,
    /// total number of disks
    total_disks: u32,
}

impl EndOfCentralDirectory64Locator {
    const LENGTH: usize = 20;
    const SIGNATURE: &'static str = "PK\x06\x07";

    fn parse<'a, E: ParseError<&'a [u8]>>(i: &'a [u8]) -> IResult<&'a [u8], Self, E> {
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
struct EndOfCentralDirectory64Record {
    /// size of zip64 end of central directory record
    record_size: u64,
    /// version made by
    creator_version: u16,
    /// version needed to extract
    reader_version: u16,
    /// number of this disk
    disk_nbr: u32,
    /// number of the disk with the start of the central directory
    dir_disk_nbr: u32,
    // total number of entries in the central directory on this disk
    dir_records_this_disk: u64,
    // total number of entries in the central directory
    directory_records: u64,
    // size of the central directory
    directory_size: u64,
    // offset of the start of central directory with respect to the
    // starting disk number
    directory_offset: u64,
}

impl EndOfCentralDirectory64Record {
    const LENGTH: usize = 56;
    const SIGNATURE: &'static str = "PK\x06\x06";

    fn read<R: ReadAt>(reader: R, directory_end_offset: u64) -> Result<Option<(u64, Self)>, Error> {
        if directory_end_offset < EndOfCentralDirectory64Locator::LENGTH as u64 {
            // no need to look for a header outside the file
            return Ok(None);
        }
        let loc_offset = directory_end_offset - EndOfCentralDirectory64Locator::LENGTH as u64;

        let mut locbuf = vec![0u8; EndOfCentralDirectory64Locator::LENGTH];
        reader.read_exact_at(loc_offset, &mut locbuf)?;
        let locres = EndOfCentralDirectory64Locator::parse::<ZipParseError>(&locbuf[..]);

        if let Ok((_, locator)) = locres {
            if locator.dir_disk_number != 0 {
                // the file is not a valid zip64 file
                return Ok(None);
            }

            if locator.total_disks != 1 {
                // the file is not a valid zip64 file
                return Ok(None);
            }

            let offset = locator.directory_offset;
            let mut recbuf = vec![0u8; EndOfCentralDirectory64Record::LENGTH];
            reader.read_exact_at(offset, &mut recbuf)?;
            let recres = Self::parse::<ZipParseError>(&recbuf[..]);

            if let Ok((_, record)) = recres {
                return Ok(Some((offset, record)));
            }
        }

        Ok(None)
    }

    fn parse<'a, E: ParseError<&'a [u8]>>(
        i: &'a [u8],
    ) -> IResult<&'a [u8], EndOfCentralDirectory64Record, E> {
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
/// Coalesces zip and zip64 "end of central directory" record info
struct EndOfCentralDirectory {
    dir: EndOfCentralDirectoryRecord,
    dir64: Option<EndOfCentralDirectory64Record>,
    global_offset: i64,
}

impl EndOfCentralDirectory {
    fn read<R: ReadAt>(reader: R, size: u64) -> Result<Self, Error> {
        let (d_offset, d) = EndOfCentralDirectoryRecord::read(&reader, size)?
            .ok_or(FormatError::DirectoryEndSignatureNotFound)?;

        // These values mean that the file can be a zip64 file
        //
        // However, on macOS, some .zip files have a zip64 directory
        // but doesn't have these values, cf. https://github.com/itchio/butler/issues/141
        let probably_zip64 = d.directory_records == 0xffff
            || d.directory_size == 0xffff
            || d.directory_offset == 0xffff;

        let mut d64_info: Option<(u64, EndOfCentralDirectory64Record)> = None;

        let res64 = EndOfCentralDirectory64Record::read(&reader, d_offset);
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
            Some((d64_offset, d64)) => *d64_offset - d64.directory_size,
            None => d_offset - d.directory_size as u64,
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

        let mut res = Self {
            dir: d,
            dir64: d64_info.map(|(_offset, record)| record),
            global_offset: 0,
        };

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

    fn directory_offset(&self) -> u64 {
        match self.dir64.as_ref() {
            Some(d64) => d64.directory_offset,
            None => self.dir.directory_offset as u64,
        }
    }

    fn set_directory_offset(&mut self, offset: u64) {
        match self.dir64.as_mut() {
            Some(d64) => d64.directory_offset = offset,
            None => self.dir.directory_offset = offset as u32,
        };
    }

    fn directory_records(&self) -> u64 {
        match self.dir64.as_ref() {
            Some(d64) => d64.directory_records,
            None => self.dir.directory_records as u64,
        }
    }

    fn comment(&self) -> &ZipString {
        &self.dir.comment
    }
}

pub fn zip_string<'a, C, E>(count: C) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], ZipString, E>
where
    C: nom::ToUsize,
    E: ParseError<&'a [u8]>,
{
    move |i: &'a [u8]| {
        map(take(count.to_usize()), |slice: &'a [u8]| {
            ZipString::from(slice)
        })(i)
    }
}

pub fn zip_bytes<'a, C, E>(count: C) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], ZipBytes, E>
where
    C: nom::ToUsize,
    E: ParseError<&'a [u8]>,
{
    move |i: &'a [u8]| {
        map(take(count.to_usize()), |slice: &'a [u8]| {
            ZipBytes(slice.into())
        })(i)
    }
}

#[derive(Debug)]
pub struct ZipReader {
    size: u64,
    encoding: Encoding,
    entries: Vec<FileHeader>,
    comment: Option<String>,
}

impl ZipReader {
    pub fn new<R>(reader: R, size: u64) -> Result<Self, Error>
    where
        R: ReadAt,
    {
        let directory_end = EndOfCentralDirectory::read(&reader, size)?;

        if directory_end.directory_records() > size / LocalFileHeaderRecord::LENGTH as u64 {
            return Err(FormatError::ImpossibleNumberOfFiles {
                claimed_records_count: directory_end.directory_records(),
                zip_size: size,
            }
            .into());
        }

        let mut header_records = Vec::<FileHeaderRecord>::new();

        {
            let mut reader = Cursor::new_pos(&reader, directory_end.directory_offset());
            let mut b = circular::Buffer::with_capacity(1000);

            'read_headers: loop {
                let sz = reader.read(b.space())?;
                b.fill(sz);

                let length = {
                    let res = FileHeaderRecord::parse::<ZipParseError>(b.data());
                    match res {
                        Ok((remaining, h)) => {
                            debug!("Parsed header: {:#?}", h);
                            header_records.push(h);
                            b.data().offset(remaining)
                        }
                        Err(e) => break 'read_headers,
                    }
                };
                b.consume(length);
            }
        }

        let mut detector = chardet::UniversalDetector::new();
        let mut all_utf8 = true;

        {
            let max_feed: usize = 4096;
            let mut total_fed: usize = 0;
            let mut feed = |slice: &[u8]| {
                detector.feed(slice);
                total_fed += slice.len();
                total_fed < max_feed
            };

            'recognize_encoding: for fh in header_records.iter().filter(|fh| fh.is_non_utf8()) {
                all_utf8 = false;
                if (!feed(&fh.name.0) || !feed(&fh.comment.0)) {
                    break 'recognize_encoding;
                }
            }
        }

        let encoding = {
            if all_utf8 {
                Encoding::Utf8
            } else {
                let (charset, confidence, _language) = detector.close();
                let label = chardet::charset2encoding(&charset);
                debug!("Detected charset {} with confidence {}", label, confidence);

                match label {
                    "SHIFT_JIS" => Encoding::ShiftJis,
                    "utf-8" => Encoding::Utf8,
                    _ => Encoding::Cp437,
                }
            }
        };

        let entries: Result<Vec<FileHeader>, encoding::DecodingError> = header_records
            .into_iter()
            .map(|x| x.as_file_header(encoding))
            .collect();
        let entries = entries?;

        let mut comment: Option<String> = None;
        if !directory_end.comment().0.is_empty() {
            comment = Some(encoding.decode(&directory_end.comment().0)?);
        }

        Ok(Self {
            size,
            entries,
            encoding,
            comment,
        })
    }

    /// Return a list of all files in this zip, read from the
    /// central directory.
    pub fn entries(&self) -> &[FileHeader] {
        &self.entries[..]
    }

    /// Returns the detected character encoding for text fields
    /// (paths, comments) inside this ZIP file
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    pub fn comment(&self) -> Option<&String> {
        self.comment.as_ref()
    }

    pub fn by_name<N: AsRef<str>>(&self, name: N) -> Option<&FileHeader> {
        self.entries.iter().find(|&x| x.name == name.as_ref())
    }
}

pub struct ArchiveReader {
    // Size of the entire zip file
    size: u64,
    state: ArchiveReaderState,
    buffer: circular::Buffer,
}

#[derive(Debug)]
pub enum ArchiveReaderResult {
    /// should continue
    Continue,
    /// done reading
    Done,
}

enum ArchiveReaderState {
    ReadEOCD {
        haystack_size: u64,
    },
    ReadEOCD64Locator {
        eocd_offset: u64,
        eocd_record: EndOfCentralDirectoryRecord,
    },
    ReadEOCD64 {
        eocd_offset: u64,
        eocd_64_offset: u64,
        eocd_record: EndOfCentralDirectoryRecord,
    },
    ReadCentralDirectory {
        eocd_offset: u64,
        eocd: EndOfCentralDirectory,
    },
}

impl ArchiveReader {
    pub fn new(size: u64) -> Self {
        let haystack_size: u64 = 65 * 1024;
        let haystack_size = if size < haystack_size {
            size
        } else {
            haystack_size
        };

        Self {
            size,
            state: ArchiveReaderState::ReadEOCD { haystack_size },
            buffer: circular::Buffer::with_capacity(128 * 1024), // 128KB buffer
        }
    }

    pub fn wants_read(&self) -> Option<u64> {
        let avail_bytes = self.buffer.available_data() as u64;

        use ArchiveReaderState as S;
        match self.state {
            S::ReadEOCD { haystack_size } => {
                if avail_bytes < haystack_size {
                    let offset = self.size - haystack_size + avail_bytes;
                    Some(offset)
                } else {
                    None
                }
            }
            _ => unimplemented!(),
        }
    }

    pub fn read(&mut self, rd: &mut Read) -> Result<usize, std::io::Error> {
        match rd.read(self.buffer.space()) {
            Ok(written) => {
                self.buffer.fill(written);
                Ok(written)
            }
            Err(e) => Err(e),
        }
    }

    pub fn process(&mut self) -> Result<ArchiveReaderResult, Error> {
        use ArchiveReaderResult as R;
        use ArchiveReaderState as S;
        match self.state {
            S::ReadEOCD { haystack_size } => {
                if (self.buffer.available_data() as u64) < haystack_size {
                    println!("ReadEnd needs more data");
                } else {
                    println!("Ok, should find EOCD now");
                    let haystack = self.buffer.data();

                    if let Some((eocd_offset_in_haystack, eocd_record)) =
                        EndOfCentralDirectoryRecord::find_in_block(haystack)
                    {
                        println!(
                            "Found (and read) EOCD, it's at {} in block",
                            eocd_offset_in_haystack
                        );
                        let eocd_offset =
                            self.size - haystack.len() as u64 + eocd_offset_in_haystack as u64;
                        println!("Its offset in the file is {}", eocd_offset);
                        println!("Here it is: {:#?}", eocd_record);
                        self.state = S::ReadEOCD64Locator {
                            eocd_offset,
                            eocd_record,
                        }
                    } else {
                        return Err(Error::Format(FormatError::DirectoryEndSignatureNotFound));
                    }
                }
                Ok(R::Continue)
            }
            _ => unimplemented!(),
        }
    }
}
