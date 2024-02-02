use tracing::trace;
use winnow::{
    binary::{le_u16, le_u32, le_u64, le_u8, length_take},
    combinator::{cond, opt, preceded, repeat_till},
    error::{ErrMode, ErrorKind, ParserError, StrContext},
    seq,
    token::{tag, take},
    PResult, Parser, Partial,
};

use crate::parse::{NtfsTimestamp, ZipBytes};

/// 4.4.28 extra field: (Variable)
pub(crate) struct ExtraFieldRecord<'a> {
    pub(crate) tag: u16,
    pub(crate) payload: &'a [u8],
}

impl<'a> ExtraFieldRecord<'a> {
    pub(crate) fn parser(i: &mut Partial<&'a [u8]>) -> PResult<Self> {
        seq! {Self {
            tag: le_u16,
            payload: length_take(le_u16),
        }}
        .parse_next(i)
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
#[derive(Debug, Clone, Copy)]
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
    Unknown {
        /// tag of the extra field
        tag: u16,
    },
}

impl ExtraField {
    pub(crate) fn mk_parser(
        settings: ExtraFieldSettings,
    ) -> impl FnMut(&mut Partial<&'_ [u8]>) -> PResult<Self> {
        move |i| {
            use ExtraField as EF;
            let rec = ExtraFieldRecord::parser.parse_next(i)?;
            trace!("parsing extra field record, tag {:04x}", rec.tag);
            let payload = &mut Partial::new(rec.payload);

            let variant = match rec.tag {
                ExtraZip64Field::TAG => opt(ExtraZip64Field::mk_parser(settings).map(EF::Zip64))
                    .context(StrContext::Label("zip64"))
                    .parse_next(payload)?,
                ExtraTimestampField::TAG => opt(ExtraTimestampField::parser.map(EF::Timestamp))
                    .context(StrContext::Label("timestamp"))
                    .parse_next(payload)?,
                ExtraNtfsField::TAG => {
                    opt(ExtraNtfsField::parse.map(EF::Ntfs)).parse_next(payload)?
                }
                ExtraUnixField::TAG | ExtraUnixField::TAG_INFOZIP => {
                    opt(ExtraUnixField::parser.map(EF::Unix)).parse_next(payload)?
                }
                ExtraNewUnixField::TAG => {
                    opt(ExtraNewUnixField::parser.map(EF::NewUnix)).parse_next(payload)?
                }
                _ => None,
            }
            .unwrap_or(EF::Unknown { tag: rec.tag });

            Ok(variant)
        }
    }
}

/// 4.5.3 -Zip64 Extended Information Extra Field (0x0001)
#[derive(Clone, Default)]
pub struct ExtraZip64Field {
    /// 64-bit uncompressed size
    pub uncompressed_size: Option<u64>,

    /// 64-bit compressed size
    pub compressed_size: Option<u64>,

    /// 64-bit header offset
    pub header_offset: Option<u64>,
}

impl ExtraZip64Field {
    const TAG: u16 = 0x0001;

    pub(crate) fn mk_parser(
        settings: ExtraFieldSettings,
    ) -> impl FnMut(&mut Partial<&'_ [u8]>) -> PResult<Self> {
        move |i| {
            // N.B: we ignore "disk start number"
            seq! {Self {
                uncompressed_size: cond(settings.needs_uncompressed_size, le_u64),
                compressed_size: cond(settings.needs_compressed_size, le_u64),
                header_offset: cond(settings.needs_header_offset, le_u64),
            }}
            .parse_next(i)
        }
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

    fn parser(i: &mut Partial<&'_ [u8]>) -> PResult<Self> {
        preceded(
            // 1 byte of flags, if bit 0 is set, modification time is present
            le_u8.verify(|x| x & 0b1 != 0),
            seq! {Self { mtime: le_u32 }},
        )
        .parse_next(i)
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

    fn parser(i: &mut Partial<&'_ [u8]>) -> PResult<Self> {
        let t_size = le_u16.parse_next(i)? - 12;
        seq! {Self {
            atime: le_u32,
            mtime: le_u32,
            uid: le_u16,
            gid: le_u16,
            data: ZipBytes::parser(t_size),
        }}
        .parse_next(i)
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
    /// file user id
    pub uid: u64,

    /// file group id
    pub gid: u64,
}

impl ExtraNewUnixField {
    const TAG: u16 = 0x7875;

    fn parser(i: &mut Partial<&'_ [u8]>) -> PResult<Self> {
        let _ = tag("\x01").parse_next(i)?;
        seq! {Self {
            uid: Self::parse_variable_length_integer,
            gid: Self::parse_variable_length_integer,
        }}
        .parse_next(i)
    }

    fn parse_variable_length_integer(i: &mut Partial<&'_ [u8]>) -> PResult<u64> {
        let slice = length_take(le_u8).parse_next(i)?;
        if let Some(u) = match slice.len() {
            1 => Some(le_u8.parse_peek(slice)?.1 as u64),
            2 => Some(le_u16.parse_peek(slice)?.1 as u64),
            4 => Some(le_u32.parse_peek(slice)?.1 as u64),
            8 => Some(le_u64.parse_peek(slice)?.1),
            _ => None,
        } {
            Ok(u)
        } else {
            Err(ErrMode::from_error_kind(i, ErrorKind::Alt))
        }
    }
}

/// 4.5.5 -NTFS Extra Field (0x000a):
#[derive(Clone)]
pub struct ExtraNtfsField {
    /// NTFS attributes
    pub attrs: Vec<NtfsAttr>,
}

impl ExtraNtfsField {
    const TAG: u16 = 0x000a;

    fn parse(i: &mut Partial<&'_ [u8]>) -> PResult<Self> {
        let _ = take(4_usize).parse_next(i)?; // reserved (unused)
        seq! {Self {
            // from the winnow docs:
            //   Parsers like repeat do not know when an eof is from insufficient
            //   data or the end of the stream, causing them to always report
            //   Incomplete.
            // using repeat_till with eof combinator to work around this:
            attrs: repeat_till(0.., NtfsAttr::parse, winnow::combinator::eof).map(|x| x.0),
        }}
        .parse_next(i)
    }
}

/// NTFS attribute for zip entries (mostly timestamps)
#[derive(Clone)]
pub enum NtfsAttr {
    /// NTFS attribute 1, which contains modified/accessed/created timestamps
    Attr1(NtfsAttr1),

    /// Unknown NTFS attribute
    Unknown {
        /// tag of the attribute
        tag: u16,
    },
}

impl NtfsAttr {
    fn parse(i: &mut Partial<&'_ [u8]>) -> PResult<Self> {
        let tag = le_u16.parse_next(i)?;
        trace!("parsing NTFS attribute, tag {:04x}", tag);
        let payload = length_take(le_u16).parse_next(i)?;

        match tag {
            0x0001 => NtfsAttr1::parser
                .parse_peek(Partial::new(payload))
                .map(|(_, attr)| NtfsAttr::Attr1(attr)),
            _ => Ok(NtfsAttr::Unknown { tag }),
        }
    }
}

/// NTFS attribute 1, which contains modified/accessed/created timestamps
#[derive(Clone)]
pub struct NtfsAttr1 {
    /// modified time
    pub mtime: NtfsTimestamp,

    /// accessed time
    pub atime: NtfsTimestamp,

    /// created time
    pub ctime: NtfsTimestamp,
}

impl NtfsAttr1 {
    fn parser(i: &mut Partial<&'_ [u8]>) -> PResult<Self> {
        trace!("parsing NTFS attr 1, input len is {}", i.len());
        seq! {Self {
            mtime: NtfsTimestamp::parser,
            atime: NtfsTimestamp::parser,
            ctime: NtfsTimestamp::parser,
        }}
        .parse_next(i)
    }
}
