#![allow(unused)]
use hex_fmt::HexFmt;
use std::fmt;

// Describes a file within a zip file.
#[derive(Debug)]
pub struct FileHeader {
    // Name of the file
    // Must be a relative path, not start with a drive letter (e.g. C:),
    // and must use forward slashes instead of back slashes
    pub name: String,

    // Comment is any arbitrary user-defined string shorter than 64KiB
    pub comment: Option<String>,

    pub creator_version: u16,
    pub reader_version: u16,
    pub flags: u16,

    pub modified: chrono::DateTime<chrono::offset::Utc>,

    pub crc32: u32,
    pub compressed_size: u64,
    pub uncompressed_size: u64,

    pub extra: Option<Vec<u8>>,
    pub external_attrs: u32,
}

// Compression method
#[repr(u16)]
#[derive(Debug)]
pub enum Method {
    Store = 0,
    Deflate = 8,
    BZIP2 = 12,
    LZMA = 14,
}

pub(crate) fn zero_datetime() -> chrono::DateTime<chrono::offset::Utc> {
    chrono::DateTime::from_utc(
        chrono::naive::NaiveDateTime::from_timestamp(0, 0),
        chrono::offset::Utc,
    )
}

impl FileHeader {
    pub fn new<S>(name: S, uncompressed_size: u64, method: Method) -> Self
    where
        S: Into<String>,
    {
        Self {
            name: name.into(),
            comment: None,

            creator_version: ZipVersion::Version45 as u16,
            reader_version: ZipVersion::Version45 as u16,
            flags: 0,

            modified: zero_datetime(),

            crc32: 0,
            compressed_size: 0,
            uncompressed_size: 0,

            extra: None,
            external_attrs: 0,
        }
    }
}

/// Constants for the first byte in creator_version
#[repr(u8)]
enum CreatorVersion {
    FAT = 0,
    Unix = 3,
    NTFS = 11,
    VFAT = 14,
    MacOSX = 19,
}

/// Version numbers
#[repr(u8)]
enum ZipVersion {
    /// 2.0
    Version20 = 20,
    /// 4.5 (reads and writes zip64 archives)
    Version45 = 45,
}

#[derive(Clone)]
pub struct ZipString(pub Vec<u8>);

impl<'a> From<&'a [u8]> for ZipString {
    fn from(slice: &'a [u8]) -> Self {
        Self(slice.into())
    }
}

impl ZipString {
    pub(crate) fn as_option(self) -> Option<ZipString> {
        if self.0.len() > 0 {
            Some(self)
        } else {
            None
        }
    }
}

impl fmt::Debug for ZipString {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match std::str::from_utf8(&self.0) {
            Ok(s) => write!(f, "{:?}", s),
            Err(_) => write!(f, "[non-utf8 string: {:x}]", HexFmt(&self.0)),
        }
    }
}
#[derive(Clone)]
pub struct ZipBytes(pub Vec<u8>);

impl ZipBytes {
    pub(crate) fn as_option(self) -> Option<ZipBytes> {
        if self.0.len() > 0 {
            Some(self)
        } else {
            None
        }
    }
}

impl fmt::Debug for ZipBytes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        const MAX_SHOWN_SIZE: usize = 10;
        let data = &self.0[..];
        let (slice, extra) = if data.len() > MAX_SHOWN_SIZE {
            (&self.0[..MAX_SHOWN_SIZE], Some(data.len() - MAX_SHOWN_SIZE))
        } else {
            (&self.0[..], None)
        };
        write!(f, "{:x}", HexFmt(slice))?;
        if let Some(extra) = extra {
            write!(f, " (+ {} bytes)", extra)?;
        }
        Ok(())
    }
}
