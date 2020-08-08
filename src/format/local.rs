use crate::format::*;
use nom::{
    bytes::streaming::tag,
    combinator::opt,
    number::streaming::{le_u16, le_u32, le_u64},
    sequence::preceded,
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

    pub fn parse<'a>(i: &'a [u8]) -> parse::Result<'a, Self> {
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
                name: ZipString::parser(dbg!(name_len)),
                extra: ZipBytes::parser(dbg!(extra_len)),
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

    pub fn parse<'a>(i: &'a [u8], is_zip64: bool) -> parse::Result<'a, Self> {
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
