use chrono::{offset::TimeZone, DateTime, Utc};
use tracing::trace;
use winnow::{
    binary::{le_u16, le_u32},
    prelude::PResult,
    token::tag,
    Parser, Partial,
};

use crate::{
    encoding::detect_utf8, zero_datetime, Encoding, Entry, Error, ExtraField, ExtraFieldSettings,
    FormatError, HostSystem, Mode, MsdosMode, MsdosTimestamp, NtfsAttr, StoredEntry,
    StoredEntryInner, UnixMode, Version, ZipBytes, ZipString,
};

/// 4.3.12 Central directory structure: File header
pub struct DirectoryHeader {
    // version made by
    pub creator_version: Version,
    // version needed to extract
    pub reader_version: Version,
    // general purpose bit flag
    pub flags: u16,
    // compression method
    pub method: u16,
    // last mod file datetime
    pub modified: MsdosTimestamp,
    // crc32
    pub crc32: u32,
    // compressed size
    pub compressed_size: u32,
    // uncompressed size
    pub uncompressed_size: u32,
    // disk number start
    pub disk_nbr_start: u16,
    // internal file attributes
    pub internal_attrs: u16,
    // external file attributes
    pub external_attrs: u32,
    // relative offset of local header
    pub header_offset: u32,

    // name
    pub name: ZipString,
    // extra
    pub extra: ZipBytes, // comment
    pub comment: ZipString,
}

impl DirectoryHeader {
    const SIGNATURE: &'static str = "PK\x01\x02";

    pub fn parser(i: &mut Partial<&'_ [u8]>) -> PResult<Self> {
        _ = tag(Self::SIGNATURE).parse_next(i)?;
        let creator_version = Version::parser.parse_next(i)?;
        let reader_version = Version::parser.parse_next(i)?;
        let flags = le_u16.parse_next(i)?;
        let method = le_u16.parse_next(i)?;
        let modified = MsdosTimestamp::parser.parse_next(i)?;
        let crc32 = le_u32.parse_next(i)?;
        let compressed_size = le_u32.parse_next(i)?;
        let uncompressed_size = le_u32.parse_next(i)?;
        let name_len = le_u16.parse_next(i)?;
        let extra_len = le_u16.parse_next(i)?;
        let comment_len = le_u16.parse_next(i)?;
        let disk_nbr_start = le_u16.parse_next(i)?;
        let internal_attrs = le_u16.parse_next(i)?;
        let external_attrs = le_u32.parse_next(i)?;
        let header_offset = le_u32.parse_next(i)?;

        let name = ZipString::parser(name_len).parse_next(i)?;
        let extra = ZipBytes::parser(extra_len).parse_next(i)?;
        let comment = ZipString::parser(comment_len).parse_next(i)?;

        Ok(Self {
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
            name,
            extra,
            comment,
        })
    }
}

impl DirectoryHeader {
    pub fn is_non_utf8(&self) -> bool {
        let (valid1, require1) = detect_utf8(&self.name.0[..]);
        let (valid2, require2) = detect_utf8(&self.comment.0[..]);
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

    pub fn as_stored_entry(
        &self,
        is_zip64: bool,
        encoding: Encoding,
        global_offset: u64,
    ) -> Result<StoredEntry, Error> {
        let mut comment: Option<String> = None;
        if let Some(comment_field) = self.comment.clone().into_option() {
            comment = Some(encoding.decode(&comment_field.0)?);
        }

        let name = encoding.decode(&self.name.0)?;

        let mut compressed_size = self.compressed_size as u64;
        let mut uncompressed_size = self.uncompressed_size as u64;
        let mut header_offset = self.header_offset as u64 + global_offset;

        let mut modified: Option<DateTime<Utc>> = None;
        let mut created: Option<DateTime<Utc>> = None;
        let mut accessed: Option<DateTime<Utc>> = None;

        let mut uid: Option<u32> = None;
        let mut gid: Option<u32> = None;

        let mut extra_fields: Vec<ExtraField> = Vec::new();

        let settings = ExtraFieldSettings {
            needs_compressed_size: self.compressed_size == !0u32,
            needs_uncompressed_size: self.uncompressed_size == !0u32,
            needs_header_offset: self.header_offset == !0u32,
        };

        let mut slice = Partial::new(&self.extra.0[..]);
        while !slice.is_empty() {
            match ExtraField::mk_parser(settings).parse_next(&mut slice) {
                Ok(ef) => {
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
                            modified = Utc.timestamp_opt(ts.mtime as i64, 0).single();
                        }
                        ExtraField::Ntfs(nf) => {
                            for attr in &nf.attrs {
                                // note: other attributes are unsupported
                                if let NtfsAttr::Attr1(attr) = attr {
                                    modified = attr.mtime.to_datetime();
                                    created = attr.ctime.to_datetime();
                                    accessed = attr.atime.to_datetime();
                                }
                            }
                        }
                        ExtraField::Unix(uf) => {
                            modified = Utc.timestamp_opt(uf.mtime as i64, 0).single();
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
                }
                Err(e) => {
                    trace!("extra field error: {:#?}", e);
                    return Err(FormatError::InvalidExtraField.into());
                }
            }
        }

        let modified = match modified {
            Some(m) => Some(m),
            None => self.modified.to_datetime(),
        };

        let mut mode: Mode = match self.creator_version.host_system() {
            HostSystem::Unix | HostSystem::Osx => UnixMode(self.external_attrs >> 16).into(),
            HostSystem::WindowsNtfs | HostSystem::Vfat | HostSystem::MsDos => {
                MsdosMode(self.external_attrs).into()
            }
            _ => Mode(0),
        };
        if name.ends_with('/') {
            // believe it or not, this is straight from the APPNOTE
            mode |= Mode::DIR
        };

        Ok(StoredEntry {
            entry: Entry {
                name,
                method: self.method.into(),
                comment,
                modified: modified.unwrap_or_else(zero_datetime),
                created,
                accessed,
            },

            creator_version: self.creator_version,
            reader_version: self.reader_version,
            flags: self.flags,

            inner: StoredEntryInner {
                crc32: self.crc32,
                compressed_size,
                uncompressed_size,
                is_zip64,
            },
            header_offset,

            uid,
            gid,
            mode,

            extra_fields,

            external_attrs: self.external_attrs,
        })
    }
}
