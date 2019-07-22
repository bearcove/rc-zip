use crate::format::*;
use hex_fmt::HexFmt;
use nom::{
    bytes::streaming::{tag, take},
    combinator::{cond, map, verify},
    multi::{length_data, many0},
    number::streaming::{le_u16, le_u32, le_u64, le_u8},
    sequence::{preceded, tuple},
};
use std::fmt;

/// 4.4.28 extra field: (Variable)
pub(crate) struct ExtraFieldRecord<'a> {
    pub(crate) tag: u16,
    pub(crate) payload: &'a [u8],
}

impl<'a> fmt::Debug for ExtraFieldRecord<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "tag 0x{:x}: {}", self.tag, HexFmt(self.payload))
    }
}

impl<'a> ExtraFieldRecord<'a> {
    pub(crate) fn parse(i: &'a [u8]) -> parse::Result<'a, Self> {
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
pub(crate) struct ExtraFieldSettings {
    pub(crate) needs_uncompressed_size: bool,
    pub(crate) needs_compressed_size: bool,
    pub(crate) needs_header_offset: bool,
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
    pub(crate) fn parse<'a>(i: &'a [u8], settings: &ExtraFieldSettings) -> parse::Result<'a, Self> {
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

    pub(crate) fn parse<'a>(i: &'a [u8], settings: &ExtraFieldSettings) -> parse::Result<'a, Self> {
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
    pub mtime: u32,
}

impl ExtraTimestampField {
    const TAG: u16 = 0x5455;

    fn parse<'a>(i: &'a [u8]) -> parse::Result<'a, Self> {
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

    fn parse<'a>(i: &'a [u8]) -> parse::Result<'a, Self> {
        let (i, t_size) = le_u16(i)?;
        let t_size = t_size - 12;
        fields!(Self {
            atime: le_u32,
            mtime: le_u32,
            uid: le_u16,
            gid: le_u16,
            data: ZipBytes::parser(t_size),
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

    fn parse<'a>(i: &'a [u8]) -> parse::Result<'a, Self> {
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

    fn parse_variable_length_integer<'a>(i: &'a [u8]) -> parse::Result<'a, u64> {
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
    pub attrs: Vec<NtfsAttr>,
}

impl ExtraNtfsField {
    const TAG: u16 = 0x000a;

    fn parse<'a>(i: &'a [u8]) -> parse::Result<'a, Self> {
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
    fn parse<'a>(i: &'a [u8]) -> parse::Result<'a, Self> {
        let (i, (tag, payload)) = tuple((le_u16, length_data(le_u16)))(i)?;
        match tag {
            0x0001 => NtfsAttr1::parse(payload).map(|(i, x)| (i, NtfsAttr::Attr1(x))),
            _ => Ok((i, NtfsAttr::Unknown { tag })),
        }
    }
}

#[derive(Debug)]
pub struct NtfsAttr1 {
    pub mtime: NtfsTimestamp,
    pub atime: NtfsTimestamp,
    pub ctime: NtfsTimestamp,
}

impl NtfsAttr1 {
    fn parse<'a>(i: &'a [u8]) -> parse::Result<'a, Self> {
        fields!(Self {
            mtime: NtfsTimestamp::parse,
            atime: NtfsTimestamp::parse,
            ctime: NtfsTimestamp::parse,
        })(i)
    }
}
