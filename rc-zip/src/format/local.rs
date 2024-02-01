use crate::format::*;
use winnow::{
    binary::{le_u16, le_u32, le_u64},
    combinator::opt,
    seq,
    token::tag,
    PResult, Parser, Partial,
};

#[derive(Debug)]
/// 4.3.7 Local file header
pub struct LocalFileHeaderRecord {
    /// version needed to extract
    pub reader_version: Version,
    /// general purpose bit flag
    pub flags: u16,
    /// compression method
    pub method: u16,
    /// last mod file datetime
    pub modified: MsdosTimestamp,
    /// crc-32
    pub crc32: u32,
    /// compressed size
    pub compressed_size: u32,
    /// uncompressed size
    pub uncompressed_size: u32,
    // file name
    pub name: ZipString,
    // extra field
    pub extra: ZipBytes,
}

impl LocalFileHeaderRecord {
    pub const SIGNATURE: &'static str = "PK\x03\x04";

    pub fn parser(i: &mut Partial<&'_ [u8]>) -> PResult<Self> {
        let _ = tag(Self::SIGNATURE).parse_next(i)?;

        let reader_version = Version::parser.parse_next(i)?;
        let flags = le_u16.parse_next(i)?;
        let method = le_u16.parse_next(i)?;
        let modified = MsdosTimestamp::parser.parse_next(i)?;
        let crc32 = le_u32.parse_next(i)?;
        let compressed_size = le_u32.parse_next(i)?;
        let uncompressed_size = le_u32.parse_next(i)?;

        let name_len = le_u16.parse_next(i)?;
        let extra_len = le_u16.parse_next(i)?;

        let name = ZipString::parser(name_len).parse_next(i)?;
        let extra = ZipBytes::parser(extra_len).parse_next(i)?;

        Ok(Self {
            reader_version,
            flags,
            method,
            modified,
            crc32,
            compressed_size,
            uncompressed_size,
            name,
            extra,
        })
    }

    pub fn has_data_descriptor(&self) -> bool {
        // 4.3.9.1 This descriptor MUST exist if bit 3 of the general
        // purpose bit flag is set (see below).
        self.flags & 0b1000 != 0
    }
}

/// 4.3.9  Data descriptor:
#[derive(Debug)]
pub struct DataDescriptorRecord {
    /// CRC32 checksum
    pub crc32: u32,
    /// Compressed size
    pub compressed_size: u64,
    /// Uncompressed size
    pub uncompressed_size: u64,
}

impl DataDescriptorRecord {
    const SIGNATURE: &'static str = "PK\x07\x08";

    pub fn mk_parser(is_zip64: bool) -> impl FnMut(&mut Partial<&'_ [u8]>) -> PResult<Self> {
        move |i| {
            // From appnote.txt:
            //
            // 4.3.9.3 Although not originally assigned a signature, the value
            // 0x08074b50 has commonly been adopted as a signature value for the
            // data descriptor record.  Implementers SHOULD be aware that ZIP files
            // MAY be encountered with or without this signature marking data
            // descriptors and SHOULD account for either case when reading ZIP files
            // to ensure compatibility.
            let _ = opt(tag(Self::SIGNATURE)).parse_next(i)?;

            if is_zip64 {
                seq! {Self {
                    crc32: le_u32,
                    compressed_size: le_u64,
                    uncompressed_size: le_u64,
                }}
                .parse_next(i)
            } else {
                seq! {Self {
                    crc32: le_u32,
                    compressed_size: le_u32.map(|x| x as u64),
                    uncompressed_size: le_u32.map(|x| x as u64),
                }}
                .parse_next(i)
            }
        }
    }
}
