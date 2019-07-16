use hex_fmt::HexFmt;
use std::fmt;

use chrono::{offset::Utc, DateTime};

// Describes a file within a zip file.
#[derive(Debug)]
pub struct Entry {
    // Name of the file
    // Must be a relative path, not start with a drive letter (e.g. C:),
    // and must use forward slashes instead of back slashes
    pub name: String,

    /// Compression method
    pub method: Method,

    /// Comment is any arbitrary user-defined string shorter than 64KiB
    pub comment: Option<String>,

    /// Modified timestamp
    pub modified: chrono::DateTime<chrono::offset::Utc>,

    /// Created timestamp
    pub created: Option<chrono::DateTime<chrono::offset::Utc>>,

    /// Accessed timestamp
    pub accessed: Option<chrono::DateTime<chrono::offset::Utc>>,
}

#[derive(Debug)]
pub struct StoredEntry {
    /// Entry information
    pub entry: Entry,

    /// CRC-32 hash
    pub crc32: u32,

    // offset of the header in the zip file
    pub header_offset: u64,

    /// Size, after compression
    pub compressed_size: u64,

    /// Size, before compression
    pub uncompressed_size: u64,

    /// External attributes (zip)
    pub external_attrs: u32,

    /// Version made by
    pub creator_version: u16,

    /// Version needed to extract
    pub reader_version: u16,

    /// General purpose bit flag
    pub flags: u16,

    /// Unix user ID
    pub uid: Option<u32>,

    /// Unix group ID
    pub gid: Option<u32>,

    /// Any extra fields found while reading
    pub extra_fields: Vec<super::reader::ExtraField>,
}

impl StoredEntry {
    pub fn name(&self) -> &str {
        self.entry.name.as_ref()
    }

    pub fn comment(&self) -> Option<&str> {
        self.entry.comment.as_ref().map(|x| x.as_ref())
    }

    pub fn method(&self) -> Method {
        self.entry.method
    }

    pub fn modified(&self) -> DateTime<Utc> {
        self.entry.modified
    }

    pub fn created(&self) -> Option<&DateTime<Utc>> {
        self.entry.created.as_ref()
    }

    pub fn accessed(&self) -> Option<&DateTime<Utc>> {
        self.entry.accessed.as_ref()
    }
}

// Compression method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Store,
    Deflate,
    Bzip2,
    Lzma,
    Unsupported(u16),
}

impl From<u16> for Method {
    fn from(m: u16) -> Self {
        use Method::*;
        match m {
            0 => Store,
            8 => Deflate,
            12 => Bzip2,
            14 => Lzma,
            _ => Unsupported(m),
        }
    }
}

impl Into<u16> for Method {
    fn into(self) -> u16 {
        use Method::*;
        match self {
            Store => 0,
            Deflate => 8,
            Bzip2 => 12,
            Lzma => 14,
            Unsupported(m) => m,
        }
    }
}

pub(crate) fn zero_datetime() -> chrono::DateTime<chrono::offset::Utc> {
    chrono::DateTime::from_utc(
        chrono::naive::NaiveDateTime::from_timestamp(0, 0),
        chrono::offset::Utc,
    )
}

impl Entry {
    pub fn new<S>(name: S, method: Method) -> Self
    where
        S: Into<String>,
    {
        Self {
            name: name.into(),
            comment: None,
            modified: zero_datetime(),
            created: None,
            accessed: None,
            method,
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
