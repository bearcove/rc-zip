use crate::format::*;

/// An Archive contains general information about a zip files,
/// along with a list of [entries][StoredEntry].
///
/// It is obtained via an [ArchiveReader](crate::reader::ArchiveReader), or via a higher-level API
/// like the [ReadZip](crate::reader::ReadZip) trait.
#[derive(Debug)]
pub struct Archive {
    pub(crate) size: u64,
    pub(crate) encoding: Encoding,
    pub(crate) entries: Vec<StoredEntry>,
    pub(crate) comment: Option<String>,
}

impl Archive {
    /// Return a list of all files in this zip, read from the
    /// central directory.
    pub fn entries(&self) -> &[StoredEntry] {
        &self.entries[..]
    }

    /// Attempts to look up an entry by name. This is usually a bad idea,
    /// as names aren't necessarily normalized in zip archives.
    pub fn by_name<N: AsRef<str>>(&self, name: N) -> Option<&StoredEntry> {
        self.entries.iter().find(|&x| x.name() == name.as_ref())
    }

    /// Returns the detected character encoding for text fields
    /// (names, comments) inside this zip archive.
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// Returns the comment for this archive, if any. When reading
    /// a zip file with an empty comment field, this will return None.
    pub fn comment(&self) -> Option<&String> {
        self.comment.as_ref()
    }
}

/// Describes a zip archive entry (a file, a directory, a symlink)
///
/// `Entry` contains normalized metadata fields, that can be set when
/// writing a zip archive. Additional metadata, along with the information
/// required to extract an entry, are available in [StoredEntry][] instead.
#[derive(Debug)]
pub struct Entry {
    /// Name of the file
    /// Must be a relative path, not start with a drive letter (e.g. C:),
    /// and must use forward slashes instead of back slashes
    pub name: String,

    /// Compression method
    ///
    /// See [Method][] for more details.
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

/// An entry as stored into an Archive. Contains additional metadata and offset information.
///
/// Whereas [Entry][] is archive-independent, [StoredEntry][] contains information that is tied to
/// a specific archive.
///
/// When reading archives, one deals with a list of [StoredEntry][], whereas when writing one, one
/// typically only specifies an [Entry][] and provides the entry's contents: fields like the CRC32
/// hash, uncompressed size, and compressed size are derived automatically from the input.
#[derive(Debug)]
pub struct StoredEntry {
    /// Archive-independent information
    ///
    /// This contains the entry's name, timestamps, comment, compression method.
    pub entry: Entry,

    /// CRC-32 hash as found in the central directory.
    ///
    /// Note that this may be zero, and the actual CRC32 might be in the local header, or (more
    /// commonly) in the data descriptor instead.
    pub crc32: u32,

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

    /// Size in bytes, after compression
    pub compressed_size: u64,

    /// Size in bytes, before compression
    ///
    /// This will be zero for directories.
    pub uncompressed_size: u64,

    /// External attributes (zip)
    pub external_attrs: u32,

    /// Version of zip supported by the tool that crated this archive.
    pub creator_version: Version,

    /// Version of zip needed to extract this archive.
    pub reader_version: Version,

    /// General purpose bit flag
    ///
    /// In the zip format, the most noteworthy flag (bit 11) is for UTF-8 names.
    /// Other flags can indicate: encryption (unsupported), various compression
    /// settings (depending on the [Method][] used).
    pub flags: u16,

    /// Unix user ID
    ///
    /// Only present if a Unix extra field or New Unix extra field was found.
    pub uid: Option<u32>,

    /// Unix group ID
    ///
    /// Only present if a Unix extra field or New Unix extra field was found.
    pub gid: Option<u32>,

    /// File mode
    pub mode: Mode,

    /// Any extra fields recognized while parsing the file.
    ///
    /// Most of these should be normalized and accessible as other fields,
    /// but they are also made available here raw.
    pub extra_fields: Vec<ExtraField>,

    /// True if this entry was read from a zip64 archive
    pub is_zip64: bool,
}

impl StoredEntry {
    /// Returns the entry's name
    ///
    /// This should be a relative path, separated by `/`. However, there are zip files in the wild
    /// with all sorts of evil variants, so, be conservative in what you accept.
    pub fn name(&self) -> &str {
        self.entry.name.as_ref()
    }

    /// The entry's comment, if any.
    ///
    /// When reading a zip file, an empty comment results in None.
    pub fn comment(&self) -> Option<&str> {
        self.entry.comment.as_ref().map(|x| x.as_ref())
    }

    /// The compression method used for this entry
    pub fn method(&self) -> Method {
        self.entry.method
    }

    /// This entry's "last modified" timestamp - with caveats
    ///
    /// Due to the history of the ZIP file format, this may be inaccurate. It may be offset
    /// by a few hours, if there is no extended timestamp information. It may have a resolution
    /// as low as two seconds, if only MSDOS timestamps are present. It may default to the Unix
    /// epoch, if something went really wrong.
    ///
    /// If you're reading this after the year 2038, or after the year 2108, godspeed.
    pub fn modified(&self) -> DateTime<Utc> {
        self.entry.modified
    }

    /// This entry's "created" timestamp, if available.
    ///
    /// See [StoredEntry::modified()] for caveats.
    pub fn created(&self) -> Option<&DateTime<Utc>> {
        self.entry.created.as_ref()
    }

    /// This entry's "last accessed" timestamp, if available.
    ///
    /// See [StoredEntry::modified()] for caveats.
    pub fn accessed(&self) -> Option<&DateTime<Utc>> {
        self.entry.accessed.as_ref()
    }

    pub fn reader<'a, F, R>(&'a self, get_reader: F) -> crate::reader::EntryReader<'a, R>
    where
        R: std::io::Read,
        F: Fn(u64) -> R,
    {
        crate::reader::EntryReader::new(self, get_reader)
    }
}

/// The contents of an entry: a directory, a file, or a symbolic link.
#[derive(Debug)]
pub enum EntryContents<'a> {
    Directory(Directory<'a>),
    File(File<'a>),
    Symlink(Symlink<'a>),
}

impl StoredEntry {
    pub fn contents<'a>(&'a self) -> EntryContents<'a> {
        if self.mode.has(Mode::SYMLINK) {
            EntryContents::Symlink(Symlink { entry: &self })
        } else if self.mode.has(Mode::DIR) {
            EntryContents::Directory(Directory { entry: &self })
        } else {
            EntryContents::File(File { entry: &self })
        }
    }
}

#[derive(Debug)]
pub struct Directory<'a> {
    pub entry: &'a StoredEntry,
}

#[derive(Debug)]
pub struct File<'a> {
    pub entry: &'a StoredEntry,
}

#[derive(Debug)]
pub struct Symlink<'a> {
    pub entry: &'a StoredEntry,
}

/// Compression method used for a file entry.
///
/// In archives that follow [ISO/IEC 21320-1:2015](https://www.iso.org/standard/60101.html), only
/// [Store][Method::Store] and [Deflate][Method::Deflate] should be used.
///
/// However, in the wild, it is not too uncommon to encounter [Bzip2][Method::Bzip2],
/// [Lzma][Method::Lzma] or others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    /// No compression is applied
    Store,
    /// [DEFLATE (RFC 1951)](https://www.ietf.org/rfc/rfc1951.txt)
    Deflate,
    /// [BZIP-2](https://github.com/dsnet/compress/blob/master/doc/bzip2-format.pdf)
    Bzip2,
    /// [LZMA](https://github.com/jljusten/LZMA-SDK/blob/master/DOC/lzma-specification.txt)
    Lzma,
    /// A compression method that isn't supported by this crate.
    ///
    /// The original u16 is preserved.
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
