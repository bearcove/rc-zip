use chrono::{offset::Utc, DateTime, TimeZone};
use num_enum::{FromPrimitive, IntoPrimitive};
use ownable::{IntoOwned, ToOwned};
use winnow::{binary::le_u16, PResult, Partial};

use crate::{
    encoding::Encoding,
    parse::{Mode, Version},
};

use super::{zero_datetime, ExtraField, NtfsAttr};

/// An Archive contains general information about a zip files, along with a list
/// of [entries][Entry].
///
/// It is obtained through a state machine like
/// [ArchiveFsm](crate::fsm::ArchiveFsm), although end-users tend to use
/// higher-levelr interfaces like
/// [rc-zip-sync](https://crates.io/crates/rc-zip-sync) or
/// [rc-zip-tokio](https://crates.io/crates/rc-zip-tokio).
pub struct Archive {
    pub(crate) size: u64,
    pub(crate) encoding: Encoding,
    pub(crate) entries: Vec<Entry>,
    pub(crate) comment: String,
}

impl Archive {
    /// The size of .zip file that was read, in bytes.
    #[inline(always)]
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Iterate over all files in this zip, read from the central directory.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    /// Attempts to look up an entry by name. This is usually a bad idea,
    /// as names aren't necessarily normalized in zip archives.
    pub fn by_name<N: AsRef<str>>(&self, name: N) -> Option<&Entry> {
        self.entries.iter().find(|&x| x.name == name.as_ref())
    }

    /// Returns the detected character encoding for text fields
    /// (names, comments) inside this zip archive.
    #[inline(always)]
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// Returns the comment for this archive, if any. When reading
    /// a zip file with an empty comment field, this will return None.
    #[inline(always)]
    pub fn comment(&self) -> &str {
        &self.comment
    }
}

/// Describes a zip archive entry (a file, a directory, a symlink)
#[derive(Clone)]
pub struct Entry {
    /// Name of the file
    ///
    /// This should be a relative path, separated by `/`. However, there are zip
    /// files in the wild with all sorts of evil variants, so, be conservative
    /// in what you accept.
    ///
    /// See also [Self::sanitized_name], which returns a sanitized version of
    /// the name, working around zip slip vulnerabilities.
    pub name: String,

    /// Compression method: Store, Deflate, Bzip2, etc.
    pub method: Method,

    /// Comment is any arbitrary user-defined string shorter than 64KiB
    pub comment: String,

    /// This entry's "last modified" timestamp - with caveats
    ///
    /// Due to the history of the ZIP file format, this may be inaccurate. It may be offset
    /// by a few hours, if there is no extended timestamp information. It may have a resolution
    /// as low as two seconds, if only MSDOS timestamps are present. It may default to the Unix
    /// epoch, if something went really wrong.
    ///
    /// If you're reading this after the year 2038, or after the year 2108, godspeed.
    pub modified: DateTime<Utc>,

    /// This entry's "created" timestamp, if available.
    ///
    /// See [Self::modified] for caveats.
    pub created: Option<DateTime<Utc>>,

    /// This entry's "last accessed" timestamp, if available.
    ///
    /// See [Self::accessed] for caveats.
    pub accessed: Option<DateTime<Utc>>,

    /// Offset of the local file header in the zip file
    ///
    /// ```text
    /// [optional non-zip data]
    /// [local file header 1] <------ header_offset points here
    /// [encryption header 1]
    /// [file data 1]
    /// [data descriptor 1]
    /// ...
    /// [central directory]
    /// [optional zip64 end of central directory info]
    /// [end of central directory record]
    /// ```
    pub header_offset: u64,

    /// Version of zip needed to extract this archive.
    pub reader_version: Version,

    /// General purpose bit flag
    ///
    /// In the zip format, the most noteworthy flag (bit 11) is for UTF-8 names.
    /// Other flags can indicate: encryption (unsupported), various compression
    /// settings (depending on the [Method] used).
    ///
    /// For LZMA, general-purpose bit 1 denotes the EOS marker.
    pub flags: u16,

    /// Unix user ID
    ///
    /// Only present if a Unix extra field or New Unix extra field was found.
    pub uid: Option<u32>,

    /// Unix group ID
    ///
    /// Only present if a Unix extra field or New Unix extra field was found.
    pub gid: Option<u32>,

    /// CRC-32 hash as found in the central directory.
    ///
    /// Note that this may be zero, and the actual CRC32 might be in the local header, or (more
    /// commonly) in the data descriptor instead.
    pub crc32: u32,

    /// Size in bytes, after compression
    pub compressed_size: u64,

    /// Size in bytes, before compression
    ///
    /// This will be zero for directories.
    pub uncompressed_size: u64,

    /// File mode.
    pub mode: Mode,
}

impl Entry {
    /// Returns a sanitized version of the entry's name, if it
    /// seems safe. In particular, if this method feels like the
    /// entry name is trying to do a zip slip (cf.
    /// <https://snyk.io/research/zip-slip-vulnerability>), it'll return
    /// None.
    ///
    /// Other than that, it will strip any leading slashes on non-Windows OSes.
    pub fn sanitized_name(&self) -> Option<&str> {
        let name = self.name.as_str();

        // refuse entries with traversed/absolute path to mitigate zip slip
        if name.contains("..") {
            return None;
        }

        #[cfg(windows)]
        {
            if name.contains(":\\") || name.starts_with("\\") {
                return None;
            }
            Some(name)
        }

        #[cfg(not(windows))]
        {
            // strip absolute prefix on entries pointing to root path
            let mut entry_chars = name.chars();
            let mut name = name;
            while name.starts_with('/') {
                entry_chars.next();
                name = entry_chars.as_str()
            }
            Some(name)
        }
    }

    /// Apply the extra field to the entry, updating its metadata.
    pub(crate) fn set_extra_field(&mut self, ef: &ExtraField) {
        match &ef {
            ExtraField::Zip64(z64) => {
                self.uncompressed_size = z64.uncompressed_size;
                self.compressed_size = z64.compressed_size;
                self.header_offset = z64.header_offset;
            }
            ExtraField::Timestamp(ts) => {
                self.modified = Utc
                    .timestamp_opt(ts.mtime as i64, 0)
                    .single()
                    .unwrap_or_else(zero_datetime);
            }
            ExtraField::Ntfs(nf) => {
                for attr in &nf.attrs {
                    // note: other attributes are unsupported
                    if let NtfsAttr::Attr1(attr) = attr {
                        self.modified = attr.mtime.to_datetime().unwrap_or_else(zero_datetime);
                        self.created = attr.ctime.to_datetime();
                        self.accessed = attr.atime.to_datetime();
                    }
                }
            }
            ExtraField::Unix(uf) => {
                self.modified = Utc
                    .timestamp_opt(uf.mtime as i64, 0)
                    .single()
                    .unwrap_or_else(zero_datetime);

                if self.uid.is_none() {
                    self.uid = Some(uf.uid as u32);
                }

                if self.gid.is_none() {
                    self.gid = Some(uf.gid as u32);
                }
            }
            ExtraField::NewUnix(uf) => {
                self.uid = Some(uf.uid as u32);
                self.gid = Some(uf.uid as u32);
            }
            _ => {}
        };
    }
}

/// The entry's file type: a directory, a file, or a symbolic link.
#[derive(Debug)]
pub enum EntryKind {
    /// The entry is a directory
    Directory,

    /// The entry is a file
    File,

    /// The entry is a symbolic link
    Symlink,
}

impl Entry {
    /// Determine the kind of this entry based on its mode.
    pub fn kind(&self) -> EntryKind {
        if self.mode.has(Mode::SYMLINK) {
            EntryKind::Symlink
        } else if self.mode.has(Mode::DIR) {
            EntryKind::Directory
        } else {
            EntryKind::File
        }
    }
}

/// Compression method used for a file entry.
///
/// In archives that follow [ISO/IEC 21320-1:2015](https://www.iso.org/standard/60101.html), only
/// [Store][Method::Store] and [Deflate][Method::Deflate] should be used.
///
/// However, in the wild, it is not too uncommon to encounter [Bzip2][Method::Bzip2],
/// [Lzma][Method::Lzma] or others.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, IntoPrimitive, FromPrimitive, IntoOwned, ToOwned,
)]
#[repr(u16)]
pub enum Method {
    /// No compression is applied
    Store = 0,

    /// [DEFLATE (RFC 1951)](https://www.ietf.org/rfc/rfc1951.txt)
    Deflate = 8,

    /// [DEFLATE64](https://deflate64.com/)
    Deflate64 = 9,

    /// [BZIP-2](https://github.com/dsnet/compress/blob/master/doc/bzip2-format.pdf)
    Bzip2 = 12,

    /// [LZMA](https://github.com/jljusten/LZMA-SDK/blob/master/DOC/lzma-specification.txt)
    Lzma = 14,

    /// [zstd](https://datatracker.ietf.org/doc/html/rfc8878)
    Zstd = 93,

    /// [MP3](https://www.iso.org/obp/ui/#iso:std:iso-iec:11172:-3:ed-1:v1:en)
    Mp3 = 94,

    /// [XZ](https://tukaani.org/xz/xz-file-format.txt)
    Xz = 95,

    /// [JPEG](https://jpeg.org/jpeg/)
    Jpeg = 96,

    /// [WavPack](https://www.wavpack.com/)
    WavPack = 97,

    /// [PPMd](https://en.wikipedia.org/wiki/Prediction_by_partial_matching)
    Ppmd = 98,

    /// AE-x encryption marker (see Appendix E of appnote)
    Aex = 99,

    /// A compression method that isn't recognized by this crate.
    #[num_enum(catch_all)]
    Unrecognized(u16),
}

impl Method {
    /// Parse a method from a byte slice
    pub fn parser(i: &mut Partial<&[u8]>) -> PResult<Self> {
        le_u16(i).map(From::from)
    }
}
