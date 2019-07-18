use super::{
    encoding::{self, Encoding},
    error::*,
    types::*,
};
use chrono::{
    offset::{TimeZone, Utc},
    DateTime,
};
use libflate::non_blocking::deflate;
use log::*;

#[macro_use]
mod nom_macros;
mod dates;
mod read_zip;
pub use self::{dates::*, read_zip::*};

use hex_fmt::HexFmt;
use std::fmt;
use std::io::{Read, Write};

use nom::{
    bytes::complete::{tag, take},
    combinator::{cond, map, opt, verify},
    error::ParseError,
    multi::{length_data, many0},
    number::complete::{le_u16, le_u32, le_u64, le_u8},
    sequence::{preceded, tuple},
    IResult, Offset,
};

// Reference code for zip handling:
// https://github.com/itchio/arkive/blob/master/zip/reader.go

#[derive(Debug)]
/// 4.3.7 Local file header
struct LocalFileHeaderRecord {
    /// version needed to extract
    reader_version: Version,
    /// general purpose bit flag
    flags: u16,
    /// compression method
    method: u16,
    /// last mod file datetime
    modified: MsdosTimestamp,
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
    const SIGNATURE: &'static str = "PK\x03\x04";

    fn parse<'a>(i: &'a [u8]) -> ZipParseResult<'a, Self> {
        preceded(
            tag(Self::SIGNATURE),
            fields!({
                reader_version: Version::parse,
                flags: le_u16,
                method: le_u16,
                modified: MsdosTimestamp::parse,
                crc32: le_u32,
                compressed_size: le_u32,
                uncompressed_size: le_u32,
                name_len: le_u16,
                extra_len: le_u16,
            } chain fields!({
                name: zip_string(name_len),
                extra: zip_bytes(extra_len),
            } map Self {
                reader_version,
                flags,
                method,
                modified,
                crc32,
                compressed_size,
                uncompressed_size,
                name,
                extra,
            })),
        )(i)
    }
}

/// 4.3.9  Data descriptor:
#[derive(Debug)]
struct DataDescriptorRecord {
    /// CRC32 checksum
    crc32: u32,
    /// Compressed size
    compressed_size: u64,
    /// Uncompressed size
    uncompressed_size: u64,
}

impl DataDescriptorRecord {
    const SIGNATURE: &'static str = "PK\x07\x08";

    fn parse<'a>(i: &'a [u8], is_zip64: bool) -> ZipParseResult<'a, Self> {
        if is_zip64 {
            preceded(
                opt(tag(Self::SIGNATURE)),
                fields!(Self {
                    crc32: le_u32,
                    compressed_size: le_u64,
                    uncompressed_size: le_u64,
                }),
            )(i)
        } else {
            preceded(
                opt(tag(Self::SIGNATURE)),
                fields!({
                    crc32: le_u32,
                    compressed_size: le_u32,
                    uncompressed_size: le_u32,
                } map Self {
                    crc32,
                    compressed_size: compressed_size as u64,
                    uncompressed_size: uncompressed_size as u64,
                }),
            )(i)
        }
    }
}

/// 4.3.12 Central directory structure: File header
#[derive(Debug)]
struct DirectoryHeader {
    // version made by
    creator_version: Version,
    // version needed to extract
    reader_version: Version,
    // general purpose bit flag
    flags: u16,
    // compression method
    method: u16,
    // last mod file datetime
    modified: MsdosTimestamp,
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

impl DirectoryHeader {
    const SIGNATURE_LENGTH: usize = 4;
    const SIGNATURE: &'static str = "PK\x01\x02";

    fn parse<'a>(i: &'a [u8]) -> ZipParseResult<'a, Self> {
        preceded(
            tag(Self::SIGNATURE),
            fields!({
                creator_version: Version::parse,
                reader_version: Version::parse,
                flags: le_u16,
                method: le_u16,
                modified: MsdosTimestamp::parse,
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
                    modified,
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

struct ExtraFieldRecord<'a> {
    tag: u16,
    payload: &'a [u8],
}

impl<'a> fmt::Debug for ExtraFieldRecord<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "tag 0x{:x}: {}", self.tag, HexFmt(self.payload))
    }
}

impl<'a> ExtraFieldRecord<'a> {
    fn parse(i: &'a [u8]) -> ZipParseResult<'a, Self> {
        fields!(Self {
            tag: le_u16,
            payload: length_data(le_u16),
        })(i)
    }
}

// Useful because zip64 extended information extra field has fixed order *but*
// optional fields. From the appnote:
//
// If one of the size or offset fields in the Local or Central directory record
// is too small to hold the required data, a Zip64 extended information record
// is created. The order of the fields in the zip64 extended information record
// is fixed, but the fields MUST only appear if the corresponding Local or
// Central directory record field is set to 0xFFFF or 0xFFFFFFFF.
struct ExtraFieldSettings {
    needs_uncompressed_size: bool,
    needs_compressed_size: bool,
    needs_header_offset: bool,
}

/// Information stored in the central directory header `extra` field
///
/// This typically contains timestamps, file sizes and offsets, file mode, uid/gid, etc.
///
/// See `extrafld.txt` in this crate's source distribution.
#[derive(Debug)]
pub enum ExtraField {
    /// Zip64 extended information extra field
    Zip64(ExtraZip64Field),
    /// Extended timestamp
    Timestamp(ExtraTimestampField),
    /// UNIX & Info-Zip UNIX
    Unix(ExtraUnixField),
    /// New UNIX extra field
    NewUnix(ExtraNewUnixField),
    /// NTFS (Win9x/WinNT FileTimes)
    Ntfs(ExtraNtfsField),
    /// Unknown extra field, with tag
    Unknown { tag: u16 },
}

impl ExtraField {
    fn parse<'a>(i: &'a [u8], settings: &ExtraFieldSettings) -> ZipParseResult<'a, Self> {
        use ExtraField as EF;

        let (remaining, rec) = ExtraFieldRecord::parse(i)?;

        let variant = match rec.tag {
            ExtraZip64Field::TAG => {
                if let Ok((_, tag)) = ExtraZip64Field::parse(rec.payload, settings) {
                    Some(EF::Zip64(tag))
                } else {
                    None
                }
            }
            ExtraTimestampField::TAG => {
                if let Ok((_, tag)) = ExtraTimestampField::parse(rec.payload) {
                    Some(EF::Timestamp(tag))
                } else {
                    None
                }
            }
            ExtraNtfsField::TAG => {
                if let Ok((_, tag)) = ExtraNtfsField::parse(rec.payload) {
                    Some(EF::Ntfs(tag))
                } else {
                    None
                }
            }
            ExtraUnixField::TAG | ExtraUnixField::TAG_INFOZIP => {
                if let Ok((_, tag)) = ExtraUnixField::parse(rec.payload) {
                    Some(EF::Unix(tag))
                } else {
                    None
                }
            }
            ExtraNewUnixField::TAG => {
                if let Ok((_, tag)) = ExtraNewUnixField::parse(rec.payload) {
                    Some(EF::NewUnix(tag))
                } else {
                    None
                }
            }
            _ => None,
        }
        .unwrap_or(EF::Unknown { tag: rec.tag });

        Ok((remaining, variant))
    }
}

/// 4.5.3 -Zip64 Extended Information Extra Field (0x0001)
#[derive(Debug)]
pub struct ExtraZip64Field {
    pub uncompressed_size: Option<u64>,
    pub compressed_size: Option<u64>,
    pub header_offset: Option<u64>,
}

impl ExtraZip64Field {
    const TAG: u16 = 0x0001;

    fn parse<'a>(i: &'a [u8], settings: &ExtraFieldSettings) -> ZipParseResult<'a, Self> {
        // N.B: we ignore "disk start number"
        fields!(Self {
            uncompressed_size: cond(settings.needs_uncompressed_size, le_u64),
            compressed_size: cond(settings.needs_compressed_size, le_u64),
            header_offset: cond(settings.needs_header_offset, le_u64),
        })(i)
    }
}

/// Extended timestamp extra field
#[derive(Debug)]
pub struct ExtraTimestampField {
    /// number of seconds since epoch
    mtime: u32,
}

impl ExtraTimestampField {
    const TAG: u16 = 0x5455;

    fn parse<'a>(i: &'a [u8]) -> ZipParseResult<'a, Self> {
        preceded(
            // 1 byte of flags, if bit 0 is set, modification time is present
            verify(le_u8, |x| x & 0b1 != 0),
            map(le_u32, |mtime| Self { mtime }),
        )(i)
    }
}

/// 4.5.7 -UNIX Extra Field (0x000d):
#[derive(Debug)]
pub struct ExtraUnixField {
    /// file last access time
    pub atime: u32,
    /// file last modification time
    pub mtime: u32,
    /// file user id
    pub uid: u16,
    /// file group id
    pub gid: u16,
    /// variable length data field
    pub data: ZipBytes,
}

impl ExtraUnixField {
    const TAG: u16 = 0x000d;
    const TAG_INFOZIP: u16 = 0x5855;

    fn parse<'a>(i: &'a [u8]) -> ZipParseResult<'a, Self> {
        let (i, t_size) = le_u16(i)?;
        let t_size = t_size - 12;
        fields!(Self {
            atime: le_u32,
            mtime: le_u32,
            uid: le_u16,
            gid: le_u16,
            data: zip_bytes(t_size),
        })(i)
    }
}

/// Info-ZIP New Unix Extra Field:
/// ====================================
///
/// Currently stores Unix UIDs/GIDs up to 32 bits.
/// (Last Revision 20080509)
///
/// ```text
/// Value         Size        Description
/// -----         ----        -----------
/// 0x7875        Short       tag for this extra block type ("ux")
/// TSize         Short       total data size for this block
/// Version       1 byte      version of this extra field, currently 1
/// UIDSize       1 byte      Size of UID field
/// UID           Variable    UID for this entry
/// GIDSize       1 byte      Size of GID field
/// GID           Variable    GID for this entry
/// ```
#[derive(Debug)]
pub struct ExtraNewUnixField {
    pub uid: u64,
    pub gid: u64,
}

impl ExtraNewUnixField {
    const TAG: u16 = 0x7875;

    fn parse<'a>(i: &'a [u8]) -> ZipParseResult<'a, Self> {
        preceded(
            tag("\x01"),
            map(
                tuple((
                    Self::parse_variable_length_integer,
                    Self::parse_variable_length_integer,
                )),
                |(uid, gid)| Self { uid, gid },
            ),
        )(i)
    }

    fn parse_variable_length_integer<'a>(i: &'a [u8]) -> ZipParseResult<'a, u64> {
        let (i, slice) = length_data(le_u8)(i)?;
        if let Some(u) = match slice.len() {
            1 => Some(le_u8(slice)?.1 as u64),
            2 => Some(le_u16(slice)?.1 as u64),
            4 => Some(le_u32(slice)?.1 as u64),
            8 => Some(le_u64(slice)?.1),
            _ => None,
        } {
            Ok((i, u))
        } else {
            Err(nom::Err::Failure((i, nom::error::ErrorKind::OneOf)))
        }
    }
}

/// 4.5.5 -NTFS Extra Field (0x000a):
#[derive(Debug)]
pub struct ExtraNtfsField {
    attrs: Vec<NtfsAttr>,
}

impl ExtraNtfsField {
    const TAG: u16 = 0x000a;

    fn parse<'a>(i: &'a [u8]) -> ZipParseResult<'a, Self> {
        preceded(
            take(4usize), /* reserved (unused) */
            map(many0(NtfsAttr::parse), |attrs| Self { attrs }),
        )(i)
    }
}

/// NTFS attribute for zip entries (mostly timestamps)
#[derive(Debug)]
pub enum NtfsAttr {
    Attr1(NtfsAttr1),
    Unknown { tag: u16 },
}

impl NtfsAttr {
    fn parse<'a>(i: &'a [u8]) -> ZipParseResult<'a, Self> {
        let (i, (tag, payload)) = tuple((le_u16, length_data(le_u16)))(i)?;
        match tag {
            0x0001 => NtfsAttr1::parse(payload).map(|(i, x)| (i, NtfsAttr::Attr1(x))),
            _ => Ok((i, NtfsAttr::Unknown { tag })),
        }
    }
}

#[derive(Debug)]
pub struct NtfsAttr1 {
    mtime: NtfsTimestamp,
    atime: NtfsTimestamp,
    ctime: NtfsTimestamp,
}

impl NtfsAttr1 {
    fn parse<'a>(i: &'a [u8]) -> ZipParseResult<'a, Self> {
        fields!(Self {
            mtime: NtfsTimestamp::parse,
            atime: NtfsTimestamp::parse,
            ctime: NtfsTimestamp::parse,
        })(i)
    }
}

impl DirectoryHeader {
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

    fn as_stored_entry(&self, is_zip64: bool, encoding: Encoding) -> Result<StoredEntry, Error> {
        let mut comment: Option<String> = None;
        if let Some(comment_field) = self.comment.clone().as_option() {
            comment = Some(encoding.decode(&comment_field.0)?);
        }

        let name = encoding.decode(&self.name.0)?;

        let mut compressed_size = self.compressed_size as u64;
        let mut uncompressed_size = self.uncompressed_size as u64;
        let mut header_offset = self.header_offset as u64;

        let mut modified: Option<DateTime<Utc>> = None;
        let mut created: Option<DateTime<Utc>> = None;
        let mut accessed: Option<DateTime<Utc>> = None;

        let mut uid: Option<u32> = None;
        let mut gid: Option<u32> = None;

        let mut extra_fields: Vec<ExtraField> = Vec::new();

        let settings = ExtraFieldSettings {
            needs_compressed_size: self.uncompressed_size == !0u32,
            needs_uncompressed_size: self.compressed_size == !0u32,
            needs_header_offset: self.header_offset == !0u32,
        };

        let mut slice = &self.extra.0[..];
        while slice.len() > 0 {
            match ExtraField::parse(&slice[..], &settings) {
                Ok((remaining, ef)) => {
                    match &ef {
                        ExtraField::Zip64(z64) => {
                            if let Some(n) = z64.uncompressed_size {
                                uncompressed_size = n;
                            }
                            if let Some(n) = z64.compressed_size {
                                compressed_size = n;
                            }
                            if let Some(n) = z64.header_offset {
                                header_offset = n;
                            }
                        }
                        ExtraField::Timestamp(ts) => {
                            modified = Some(Utc.timestamp(ts.mtime as i64, 0));
                        }
                        ExtraField::Ntfs(nf) => {
                            for attr in &nf.attrs {
                                match attr {
                                    NtfsAttr::Attr1(attr) => {
                                        modified = attr.mtime.to_datetime();
                                        created = attr.ctime.to_datetime();
                                        accessed = attr.atime.to_datetime();
                                    }
                                    _ => {}
                                }
                            }
                        }
                        ExtraField::Unix(uf) => {
                            modified = Some(Utc.timestamp(uf.mtime as i64, 0));
                            if uid.is_none() {
                                uid = Some(uf.uid as u32);
                            }
                            if gid.is_none() {
                                gid = Some(uf.gid as u32);
                            }
                        }
                        ExtraField::NewUnix(uf) => {
                            uid = Some(uf.uid as u32);
                            gid = Some(uf.uid as u32);
                        }
                        _ => {}
                    };
                    extra_fields.push(ef);
                    slice = remaining;
                }
                Err(e) => {
                    debug!("extra field error: {:#?}", e);
                    return Err(FormatError::InvalidExtraField.into());
                }
            }
        }

        if !extra_fields.is_empty() {
            debug!("{} extra fields: {:#?}", name, extra_fields);
        }

        let modified = match modified {
            Some(m) => Some(m),
            None => self.modified.to_datetime(),
        };

        Ok(StoredEntry {
            entry: Entry {
                name,
                method: self.method.into(),
                comment,
                modified: modified.unwrap_or_else(|| zero_datetime()),
                created,
                accessed,
            },

            creator_version: self.creator_version,
            reader_version: self.reader_version,
            flags: self.flags,

            crc32: self.crc32,
            compressed_size,
            uncompressed_size,
            header_offset,

            uid,
            gid,

            extra_fields,

            external_attrs: self.external_attrs,
            is_zip64,
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
    /// Does not include comment size & comment data
    const MIN_LENGTH: usize = 20;
    const SIGNATURE: &'static str = "PK\x05\x06";

    fn find_in_block(b: &[u8]) -> Option<Located<Self>> {
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

    fn parse<'a>(i: &'a [u8]) -> ZipParseResult<'a, Self> {
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

    fn parse<'a>(i: &'a [u8]) -> ZipParseResult<'a, Self> {
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

    fn parse<'a>(i: &'a [u8]) -> ZipParseResult<'a, Self> {
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
struct Located<T> {
    offset: u64,
    inner: T,
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
struct EndOfCentralDirectory {
    dir: Located<EndOfCentralDirectoryRecord>,
    dir64: Option<Located<EndOfCentralDirectory64Record>>,
    global_offset: i64,
}

impl EndOfCentralDirectory {
    fn new(
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

    fn located_directory_offset(&self) -> u64 {
        match self.dir64.as_ref() {
            Some(d64) => d64.offset,
            None => self.dir.offset,
        }
    }

    fn directory_offset(&self) -> u64 {
        match self.dir64.as_ref() {
            Some(d64) => d64.directory_offset,
            None => self.dir.directory_offset as u64,
        }
    }

    fn directory_size(&self) -> u64 {
        match self.dir64.as_ref() {
            Some(d64) => d64.directory_size,
            None => self.dir.directory_size as u64,
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

pub(crate) fn zip_string<'a, C, E>(count: C) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], ZipString, E>
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

pub(crate) fn zip_bytes<'a, C, E>(count: C) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], ZipBytes, E>
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

/// ArchiveReader parses a valid zip archive into an [Archive][]. In particular, this struct finds
/// an end of central directory record, parses the entire central directory, detects text encoding,
/// and normalizes metadata.
pub struct ArchiveReader {
    // Size of the entire zip file
    size: u64,
    state: ArchiveReaderState,

    buffer: Buffer,
}

#[derive(Debug)]
pub enum ArchiveReaderResult {
    /// Indicates that [ArchiveReader][] has work left, and the loop should continue.
    Continue,
    /// Indicates that [ArchiveReader][] is done reading the central directory,
    /// contains an [Archive][]. Calling any method after [process()](ArchiveReader::process()) has returned
    /// `Done` will panic.
    Done(Archive),
}

enum ArchiveReaderState {
    /// Used while transitioning because ownership rules are tough.
    Transitioning,

    /// Finding and reading the end of central directory record
    ReadEocd { haystack_size: u64 },

    /// Reading the zip64 end of central directory record.
    ReadEocd64Locator {
        eocdr: Located<EndOfCentralDirectoryRecord>,
    },

    /// Reading the zip64 end of central directory record.
    ReadEocd64 {
        eocdr64_offset: u64,
        eocdr: Located<EndOfCentralDirectoryRecord>,
    },

    /// Reading all headers from the central directory
    ReadCentralDirectory {
        eocd: EndOfCentralDirectory,
        directory_headers: Vec<DirectoryHeader>,
    },

    /// Done!
    Done,
}

macro_rules! transition {
    ($state: expr => ($pattern: pat) $body: expr) => {
        $state = if let $pattern = std::mem::replace(&mut $state, S::Transitioning) {
            $body
        } else {
            unreachable!()
        };
    };
}

#[derive(Debug)]
struct ReadOp {
    offset: u64,
}

/// A wrapper around [circular::Buffer] that keeps track of how many bytes we've read since
/// initialization or the last reset.
struct Buffer {
    buffer: circular::Buffer,
    read_bytes: u64,
}

impl Read for Buffer {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.buffer.read(buf)
    }
}

impl Write for Buffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.buffer.flush()
    }
}

impl Buffer {
    /// creates a new buffer with the specified capacity
    pub fn with_capacity(size: usize) -> Self {
        Self {
            buffer: circular::Buffer::with_capacity(size),
            read_bytes: 0,
        }
    }

    /// resets the buffer (so that data() returns an empty slice,
    /// and space() returns the full capacity), along with th e
    /// read bytes counter.
    fn reset(&mut self) {
        self.read_bytes = 0;
        self.buffer.reset();
    }

    /// returns the number of read bytes since the last reset
    fn read_bytes(&self) -> u64 {
        self.read_bytes
    }

    /// returns a slice with all the available data
    fn data(&self) -> &[u8] {
        self.buffer.data()
    }

    /// returns how much data can be read from the buffer
    fn available_data(&self) -> usize {
        self.buffer.available_data()
    }

    /// returns how much data can be written from the buffer
    fn available_space(&self) -> usize {
        self.buffer.available_space()
    }

    /// advances the position tracker
    ///
    /// if the position gets past the buffer's half,
    /// this will call `shift()` to move the remaining data
    /// to the beginning of the buffer
    fn consume(&mut self, count: usize) -> usize {
        self.buffer.consume(count)
    }

    /// fill that buffer from the given Read
    fn read(&mut self, rd: &mut Read) -> Result<usize, std::io::Error> {
        if self.buffer.available_space() == 0 {
            debug!("uh oh, buffer has no available space!")
        }

        match rd.read(self.buffer.space()) {
            Ok(written) => {
                self.read_bytes += written as u64;
                self.buffer.fill(written);
                Ok(written)
            }
            Err(e) => Err(e),
        }
    }

    fn read_offset(&self, op: ReadOp) -> u64 {
        self.read_bytes + op.offset
    }
}

impl ArchiveReader {
    /// Create a new archive reader with a specified file size.
    ///
    /// Actual reading of the file is performed by calling
    /// [wants_read()](ArchiveReader::wants_read()), [read()](ArchiveReader::read()) and
    /// [process()](ArchiveReader::process()) in a loop.
    pub fn new(size: u64) -> Self {
        let haystack_size: u64 = 65 * 1024;
        let haystack_size = if size < haystack_size {
            size
        } else {
            haystack_size
        };

        Self {
            size,
            state: ArchiveReaderState::ReadEocd { haystack_size },
            buffer: Buffer::with_capacity(128 * 1024), // 128KB buffer
        }
    }

    /// Returns whether or not this reader needs more data to continue.
    ///
    /// Returns `Some(offset)` if this reader needs to read some data from `offset`.
    /// In this case, [read()](ArchiveReader::read()) should be called with a [Read]
    /// at the correct offset.
    ///
    /// Returns `None` if the reader does not need data and [process()](ArchiveReader::process())
    /// can be called directly.
    pub fn wants_read(&self) -> Option<u64> {
        self.read_op().map(|op| self.buffer.read_offset(op))
    }

    fn read_op(&self) -> Option<ReadOp> {
        use ArchiveReaderState as S;
        match self.state {
            S::ReadEocd { haystack_size } => Some(ReadOp {
                offset: self.size - haystack_size,
            }),
            S::ReadEocd64Locator { ref eocdr } => {
                let length = EndOfCentralDirectory64Locator::LENGTH as u64;
                Some(ReadOp {
                    offset: eocdr.offset - length,
                })
            }
            S::ReadEocd64 { eocdr64_offset, .. } => Some(ReadOp {
                offset: eocdr64_offset,
            }),
            S::ReadCentralDirectory { ref eocd, .. } => Some(ReadOp {
                offset: eocd.directory_offset(),
            }),
            S::Done { .. } => panic!("Called wants_read() on ArchiveReader in Done state"),
            S::Transitioning => unreachable!(),
        }
    }

    /// Reads some data from `rd` into the reader's internal buffer.
    ///
    /// Any I/O errors will be returned.
    ///
    /// If successful, this returns the number of bytes read. On success,
    /// [process()](ArchiveReader::process()) should be called next.
    pub fn read(&mut self, rd: &mut Read) -> Result<usize, std::io::Error> {
        self.buffer.read(rd)
    }

    /// Process buffered data
    ///
    /// Errors returned from process() are caused by invalid zip archives,
    /// unsupported format quirks, or implementation bugs - never I/O errors.
    ///
    /// A result of [ArchiveReaderResult::Continue] indicates one should loop again,
    /// starting with [wants_read()](ArchiveReader::wants_read()).
    ///
    /// A result of [ArchiveReaderResult::Done] contains the [Archive], and indicates that no
    /// method should ever be called again on this reader.
    pub fn process(&mut self) -> Result<ArchiveReaderResult, Error> {
        use ArchiveReaderResult as R;
        use ArchiveReaderState as S;
        match self.state {
            S::ReadEocd { haystack_size } => {
                if self.buffer.read_bytes() < haystack_size {
                    return Ok(R::Continue);
                }

                match {
                    let haystack = &self.buffer.data()[..haystack_size as usize];
                    EndOfCentralDirectoryRecord::find_in_block(haystack)
                } {
                    None => Err(FormatError::DirectoryEndSignatureNotFound.into()),
                    Some(mut eocdr) => {
                        self.buffer.reset();
                        eocdr.offset += self.size - haystack_size;

                        if eocdr.offset < EndOfCentralDirectory64Locator::LENGTH as u64 {
                            // no room for an EOCD64 locator, definitely not a zip64 file
                            self.state = S::ReadCentralDirectory {
                                eocd: EndOfCentralDirectory::new(self.size, eocdr, None)?,
                                directory_headers: vec![],
                            };
                            Ok(R::Continue)
                        } else {
                            self.buffer.reset();
                            self.state = S::ReadEocd64Locator { eocdr };
                            Ok(R::Continue)
                        }
                    }
                }
            }
            S::ReadEocd64Locator { .. } => {
                match EndOfCentralDirectory64Locator::parse(self.buffer.data()) {
                    Err(nom::Err::Incomplete(_)) => {
                        // need more data
                        Ok(R::Continue)
                    }
                    Err(nom::Err::Error(_)) | Err(nom::Err::Failure(_)) => {
                        // we don't have a zip64 end of central directory locator - that's ok!
                        self.buffer.reset();
                        transition!(self.state => (S::ReadEocd64Locator {eocdr}) {
                            S::ReadCentralDirectory {
                                eocd: EndOfCentralDirectory::new(self.size, eocdr, None)?,
                                directory_headers: vec![],
                            }
                        });
                        Ok(R::Continue)
                    }
                    Ok((_, locator)) => {
                        self.buffer.reset();
                        transition!(self.state => (S::ReadEocd64Locator {eocdr}) {
                            S::ReadEocd64 {
                                eocdr64_offset: locator.directory_offset,
                                eocdr,
                            }
                        });
                        Ok(R::Continue)
                    }
                }
            }
            S::ReadEocd64 { .. } => {
                match EndOfCentralDirectory64Record::parse(self.buffer.data()) {
                    Err(nom::Err::Incomplete(_)) => {
                        // need more data
                        Ok(R::Continue)
                    }
                    Err(nom::Err::Error(_)) | Err(nom::Err::Failure(_)) => {
                        // at this point, we really expected to have a zip64 end
                        // of central directory record, so, we want to propagate
                        // that error.
                        Err(FormatError::Directory64EndRecordInvalid.into())
                    }
                    Ok((_, eocdr64)) => {
                        self.buffer.reset();
                        transition!(self.state => (S::ReadEocd64 { eocdr, eocdr64_offset }) {
                            S::ReadCentralDirectory {
                                eocd: EndOfCentralDirectory::new(self.size, eocdr, Some(Located {
                                    offset: eocdr64_offset,
                                    inner: eocdr64
                                }))?,
                                directory_headers: vec![],
                            }
                        });
                        Ok(R::Continue)
                    }
                }
            }
            S::ReadCentralDirectory {
                ref eocd,
                ref mut directory_headers,
            } => {
                debug!(
                    "ReadCentralDirectory | process(), available: {}",
                    self.buffer.available_data()
                );
                'read_headers: while self.buffer.available_data()
                    >= DirectoryHeader::SIGNATURE_LENGTH
                {
                    match DirectoryHeader::parse(self.buffer.data()) {
                        Err(nom::Err::Incomplete(_needed)) => {
                            // need more data
                            break 'read_headers;
                        }
                        Err(nom::Err::Error(_err)) | Err(nom::Err::Failure(_err)) => {
                            let (_, kind) = _err;
                            debug!("nom error kind: {:#?}", kind);
                            match kind {
                                nom::error::ErrorKind::Eof => {
                                    // need more data
                                    break 'read_headers;
                                }
                                _ => {}
                            }

                            // this is the normal end condition when reading
                            // the central directory (due to 65536-entries non-zip64 files)
                            // let's just check a few numbers first.

                            // only compare 16 bits here
                            let expected_records = directory_headers.len() as u16;
                            let actual_records = eocd.directory_records() as u16;

                            if expected_records == actual_records {
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

                                    'recognize_encoding: for fh in
                                        directory_headers.iter().filter(|fh| fh.is_non_utf8())
                                    {
                                        all_utf8 = false;
                                        if !feed(&fh.name.0) || !feed(&fh.comment.0) {
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
                                        debug!(
                                            "Detected charset {} with confidence {}",
                                            label, confidence
                                        );

                                        match label {
                                            "SHIFT_JIS" => Encoding::ShiftJis,
                                            "utf-8" => Encoding::Utf8,
                                            _ => Encoding::Cp437,
                                        }
                                    }
                                };

                                let is_zip64 = eocd.dir64.is_some();
                                let entries: Result<Vec<StoredEntry>, Error> = directory_headers
                                    .into_iter()
                                    .map(|x| x.as_stored_entry(is_zip64, encoding))
                                    .collect();
                                let entries = entries?;

                                let mut comment: Option<String> = None;
                                if !eocd.comment().0.is_empty() {
                                    comment = Some(encoding.decode(&eocd.comment().0)?);
                                }

                                self.state = S::Done;
                                return Ok(R::Done(Archive {
                                    size: self.size,
                                    comment,
                                    entries,
                                    encoding,
                                }));
                            } else {
                                // if we read the wrong number of directory entries,
                                // error out.
                                return Err(FormatError::InvalidCentralRecord {
                                    expected: expected_records,
                                    actual: actual_records,
                                }
                                .into());
                            }
                        }
                        Ok((remaining, dh)) => {
                            let consumed = self.buffer.data().offset(remaining);
                            drop(remaining);
                            self.buffer.consume(consumed);
                            directory_headers.push(dh);
                        }
                    }
                }

                // need more data
                return Ok(R::Continue);
            }
            S::Done { .. } => panic!("Called process() on ArchiveReader in Done state"),
            S::Transitioning => unreachable!(),
        }
    }
}

enum EntryReaderState {
    ReadLocalHeader {
        buffer: circular::Buffer,
    },
    ReadData {
        header: LocalFileHeaderRecord,
        decoder: deflate::Decoder<circular::Buffer>,
        read_bytes: u64,
    },
    ReadDataDescriptor {
        header: LocalFileHeaderRecord,
        buffer: circular::Buffer,
    },
    ValidateCRC32 {
        header: LocalFileHeaderRecord,
        descriptor: Option<DataDescriptorRecord>,
    },
    Done,
    Transitioning,
}

pub enum EntryReaderResult {
    Continue,
    Done,
}

pub struct EntryReader<'a, R>
where
    R: Read,
{
    entry: &'a StoredEntry,
    rd: R,
    state: EntryReaderState,
}

impl<'a, R> Read for EntryReader<'a, R>
where
    R: Read,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use EntryReaderState as S;
        match self.state {
            S::ReadLocalHeader { ref mut buffer } => {
                if buffer.available_data() < 4 {
                    let read_bytes = self.rd.read(buffer.space())?;
                    buffer.fill(read_bytes);
                }

                match LocalFileHeaderRecord::parse(buffer.data()) {
                    Ok((remaining, header)) => {
                        let consumed = buffer.data().offset(remaining);
                        drop(remaining);
                        buffer.consume(consumed);
                        drop(buffer);

                        debug!("local file header: {:#?}", header);
                        transition!(self.state => (S::ReadLocalHeader { buffer }) {
                            let read_bytes = std::cmp::min(buffer.available_data() as u64, self.entry.compressed_size);

                            S::ReadData {
                                decoder: deflate::Decoder::new(buffer),
                                header,
                                read_bytes,
                            }
                        });
                        self.read(buf)
                    }
                    Err(_e) => Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        Error::Format(FormatError::InvalidLocalHeader),
                    )),
                }
            }
            S::ReadData {
                ref mut decoder,
                ref mut read_bytes,
                ..
            } => {
                let remaining = self.entry.compressed_size - *read_bytes;
                if remaining > 0 {
                    let buffer = decoder.as_inner_mut();
                    let avail_space = buffer.available_space() as u64;
                    if avail_space > 0 {
                        let space = if remaining < avail_space {
                            &mut buffer.space()[..remaining as usize]
                        } else {
                            buffer.space()
                        };

                        let n = self.rd.read(space)?;
                        buffer.fill(n);
                    }
                }
                match decoder.read(buf) {
                    Ok(0) => {
                        transition!(self.state => (S::ReadData {decoder, header, ..}) {
                            let buffer = decoder.into_inner();
                            // bit 3 set (0-based)
                            if header.flags & 0b1000 > 0 {
                                debug!("will read data descriptor (flags = {:x})", header.flags);
                                S::ReadDataDescriptor { buffer, header }
                            } else {
                                debug!("no data descriptor to read");
                                S::ValidateCRC32 { header, descriptor: None }
                            }
                        });
                        self.read(buf)
                    }
                    r => r,
                }
            }
            S::ReadDataDescriptor { ref mut buffer, .. } => {
                if buffer.available_data() < 4 {
                    let read_bytes = self.rd.read(buffer.space())?;
                    buffer.fill(read_bytes);
                }

                match DataDescriptorRecord::parse(buffer.data(), self.entry.is_zip64) {
                    Ok((_remaining, descriptor)) => {
                        debug!("data descriptor = {:#?}", descriptor);
                        transition!(self.state => (S::ReadDataDescriptor {buffer, header}) {
                            S::ValidateCRC32 { header, descriptor: Some(descriptor) }
                        });
                        self.read(buf)
                    }
                    Err(_e) => Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        Error::Format(FormatError::InvalidLocalHeader),
                    )),
                }
            }
            S::ValidateCRC32 {
                ref header,
                ref descriptor,
            } => {
                let mut crc32 = self.entry.crc32;
                if crc32 == 0 {
                    if let Some(descriptor) = descriptor.as_ref() {
                        crc32 = descriptor.crc32;
                    }
                    if crc32 == 0 {
                        crc32 = header.crc32
                    }
                }

                if crc32 != 0 {
                    debug!("Should check crc32: {:x}", crc32);
                    unimplemented!()
                }
                self.state = S::Done;
                self.read(buf)
            }
            S::Done => Ok(0),
            _ => unimplemented!(),
        }
    }
}

impl<'a, R> EntryReader<'a, R>
where
    R: Read,
{
    pub fn new<F>(entry: &'a StoredEntry, get_reader: F) -> Self
    where
        F: Fn(u64) -> R,
    {
        debug!("entry: {:#?}", entry);
        Self {
            entry,
            rd: get_reader(entry.header_offset),
            state: EntryReaderState::ReadLocalHeader {
                buffer: circular::Buffer::with_capacity(128 * 1024),
            },
        }
    }
}

pub struct EntryRead {}
