use crate::{Error, Method, MsdosTimestamp, UnsupportedError, Version, ZipBytes, ZipString};

use winnow::{
    binary::{le_u16, le_u32, le_u64, le_u8},
    combinator::opt,
    error::{ContextError, ErrMode, ErrorKind, FromExternalError},
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
    pub method: Method,

    /// last mod file datetime
    pub modified: MsdosTimestamp,

    /// crc-32
    pub crc32: u32,

    /// compressed size
    pub compressed_size: u32,

    /// uncompressed size
    pub uncompressed_size: u32,

    /// file name
    pub name: ZipString,

    /// extra field
    pub extra: ZipBytes,

    /// method-specific fields
    pub method_specific: MethodSpecific,
}

#[derive(Debug)]
/// Method-specific properties following the local file header
pub enum MethodSpecific {
    /// No method-specific properties
    None,

    /// LZMA properties
    Lzma(LzmaProperties),
}

impl LocalFileHeaderRecord {
    /// The signature for a local file header
    pub const SIGNATURE: &'static str = "PK\x03\x04";

    /// Parser for the local file header
    pub fn parser(i: &mut Partial<&'_ [u8]>) -> PResult<Self> {
        let _ = tag(Self::SIGNATURE).parse_next(i)?;

        let reader_version = Version::parser.parse_next(i)?;
        let flags = le_u16.parse_next(i)?;
        let method = le_u16.parse_next(i).map(Method::from)?;
        let modified = MsdosTimestamp::parser.parse_next(i)?;
        let crc32 = le_u32.parse_next(i)?;
        let compressed_size = le_u32.parse_next(i)?;
        let uncompressed_size = le_u32.parse_next(i)?;

        let name_len = le_u16.parse_next(i)?;
        let extra_len = le_u16.parse_next(i)?;

        let name = ZipString::parser(name_len).parse_next(i)?;
        let extra = ZipBytes::parser(extra_len).parse_next(i)?;

        let method_specific = match method {
            Method::Lzma => {
                let lzma_properties = LzmaProperties::parser.parse_next(i)?;
                if let Err(e) = lzma_properties.error_if_unsupported() {
                    return Err(ErrMode::Cut(ContextError::from_external_error(
                        i,
                        ErrorKind::Verify,
                        e,
                    )));
                }
                MethodSpecific::Lzma(lzma_properties)
            }
            _ => MethodSpecific::None,
        };

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
            method_specific,
        })
    }

    /// Check for the presence of the bit flag that indicates a data descriptor
    /// is present after the file data.
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

    /// Create a parser for the data descriptor record.
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

/// 5.8.5 LZMA Properties header
#[derive(Debug)]
pub struct LzmaProperties {
    /// major version
    pub major: u8,
    /// minor version
    pub minor: u8,
    /// properties size
    pub properties_size: u16,
}

impl LzmaProperties {
    /// Parser for the LZMA properties header.
    pub fn parser(i: &mut Partial<&'_ [u8]>) -> PResult<Self> {
        // Note: the actual properties (5 bytes, contains dictionary size,
        // and various other settings) is not actually read, because lzma-rs
        // reads those properties itself.

        seq! {Self {
            major: le_u8,
            minor: le_u8,
            properties_size: le_u16,
        }}
        .parse_next(i)
    }

    /// Check if the LZMA version is supported.
    pub fn error_if_unsupported(&self) -> Result<(), Error> {
        if (self.major, self.minor) != (2, 0) {
            return Err(Error::Unsupported(
                UnsupportedError::LzmaVersionUnsupported {
                    minor: self.minor,
                    major: self.major,
                },
            ));
        }

        const LZMA_PROPERTIES_SIZE: u16 = 5;
        if self.properties_size != LZMA_PROPERTIES_SIZE {
            return Err(Error::Unsupported(
                UnsupportedError::LzmaPropertiesHeaderWrongSize {
                    expected: 5,
                    actual: self.properties_size,
                },
            ));
        }

        Ok(())
    }
}
