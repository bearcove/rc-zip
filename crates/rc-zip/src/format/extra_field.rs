use crate::format::*;
use winnow::{
    binary::{le_u16, le_u32, le_u64, le_u8, length_take}, combinator::{cond, preceded, repeat}, error::{ErrMode, ErrorKind}, seq, token::{tag, take}, PResult
};
/// 4.4.28 extra field: (Variable)
pub(crate) struct ExtraFieldRecord<'a> {
    pub(crate) tag: u16,
    pub(crate) payload: &'a [u8],
}

impl<'a> ExtraFieldRecord<'a> {
    pub(crate) fn parse(i: &'a [u8]) -> parse::Result<'a, Self> {
        seq!(Self {
            tag: le_u16,
            payload: length_take(le_u16),
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
#[derive(Debug)]
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
#[derive(Clone)]
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
#[derive(Clone)]
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
#[derive(Clone)]
pub struct ExtraTimestampField {
    /// number of seconds since epoch
    pub mtime: u32,
}

impl ExtraTimestampField {
    const TAG: u16 = 0x5455;

    fn parse(i: &[u8]) -> parse::Result<'_, Self> {
        preceded(
            // 1 byte of flags, if bit 0 is set, modification time is present
            le_u8.verify(|x| x & 0b1 != 0),
            seq!(Self { mtime: le_u32 }),
        )(i)
    }
}

/// 4.5.7 -UNIX Extra Field (0x000d):
#[derive(Clone)]
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

    fn parse(i: &[u8]) -> parse::Result<'_, Self> {
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

    fn parser(i: &mut [u8]) -> PResult<Self {
        let t_size = le_u16.parse_next(i)? - 12;
        seq!{Self {
            atime: le_u32,
            mtime: le_u32,
            uid: le_u16,
            gid: le_u16,
            data: ZipBytes::parser(t_size),
        }}
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
#[derive(Clone)]
pub struct ExtraNewUnixField {
    pub uid: u64,
    pub gid: u64,
}

impl ExtraNewUnixField {
    const TAG: u16 = 0x7875;

    fn parse(i: &[u8]) -> parse::Result<'_, Self> {
        preceded(
            tag("\x01"),
            seq! {Self {
                uid: Self::parse_variable_length_integer,
                gid: Self::parse_variable_length_integer,
            }},
        )(i)
    }

    fn parse_variable_length_integer(i: &[u8]) -> parse::Result<'_, u64> {
        let (i, slice) = length_take(le_u8)(i)?;
        if let Some(u) = match slice.len() {
            1 => Some(le_u8(slice)?.1 as u64),
            2 => Some(le_u16(slice)?.1 as u64),
            4 => Some(le_u32(slice)?.1 as u64),
            8 => Some(le_u64(slice)?.1),
            _ => None,
        } {
            Ok((i, u))
        } else {
            Err(ErrMode::from_error_kind(i, ErrorKind::Alt))
        }
    }
}

/// 4.5.5 -NTFS Extra Field (0x000a):
#[derive(Clone)]
pub struct ExtraNtfsField {
    pub attrs: Vec<NtfsAttr>,
}

impl ExtraNtfsField {
    const TAG: u16 = 0x000a;

    fn parse(i: &[u8]) -> parse::Result<'_, Self> {
        preceded(
            take(4usize), /* reserved (unused) */
            repeat(0.., NtfsAttr::parse).map(|attrs| Self { attrs }),
        )(i)
    }
}

/// NTFS attribute for zip entries (mostly timestamps)
#[derive(Clone)]
pub enum NtfsAttr {
    Attr1(NtfsAttr1),
    Unknown { tag: u16 },
}

impl NtfsAttr {
    fn parse(i: &[u8]) -> parse::Result<'_, Self> {
        let (tag, payload) = seq!(le_u16, length_take(le_u16))(i)?;
        match tag {
            0x0001 => NtfsAttr1::parse(payload).map(|(i, x)| (i, NtfsAttr::Attr1(x))),
            _ => Ok((i, NtfsAttr::Unknown { tag })),
        }
    }
}

#[derive(Clone)]
pub struct NtfsAttr1 {
    pub mtime: NtfsTimestamp,
    pub atime: NtfsTimestamp,
    pub ctime: NtfsTimestamp,
}

impl NtfsAttr1 {
    fn parse(i: &[u8]) -> parse::Result<'_, Self> {
        fields!(Self {
            mtime: NtfsTimestamp::parse,
            atime: NtfsTimestamp::parse,
            ctime: NtfsTimestamp::parse,
        })(i)
    }
}
