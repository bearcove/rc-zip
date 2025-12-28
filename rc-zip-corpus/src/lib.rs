//! A corpus of zip files for testing.

use std::{fs::File, path::PathBuf};

use chrono::{DateTime, FixedOffset, TimeZone, Timelike, Utc};
use rc_zip::{
    encoding::Encoding,
    error::{Error, FormatError},
    parse::{Archive, Entry, EntryKind},
};
use temp_dir::TempDir;
use tracing::span;

pub struct Case {
    pub name: &'static str,
    pub expected_encoding: Option<Encoding>,
    pub comment: Option<&'static str>,
    pub files: Files,
    pub error: Option<Error>,
}

pub enum Files {
    ExhaustiveList(Vec<CaseFile>),
    NumFiles(usize),
}

impl Files {
    pub fn len(&self) -> usize {
        match self {
            Self::ExhaustiveList(list) => list.len(),
            Self::NumFiles(n) => *n,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for Files {
    fn default() -> Self {
        Self::NumFiles(0)
    }
}

impl From<CaseFile> for Files {
    fn from(file: CaseFile) -> Self {
        vec![file].into()
    }
}

impl From<Vec<CaseFile>> for Files {
    fn from(files: Vec<CaseFile>) -> Self {
        Self::ExhaustiveList(files)
    }
}

impl From<usize> for Files {
    fn from(num: usize) -> Self {
        Self::NumFiles(num)
    }
}

impl Default for Case {
    fn default() -> Self {
        Self {
            name: "test.zip",
            expected_encoding: None,
            comment: None,
            files: Files::default(),
            error: None,
        }
    }
}

/// This path may disappear on drop (if the zip is bz2-compressed), so be
/// careful
pub struct GuardedPath {
    pub path: PathBuf,
    _guard: Option<TempDir>,
}

impl Case {
    pub fn absolute_path(&self) -> GuardedPath {
        let path = zips_dir().join(self.name);
        if let Some(dec_name) = self.name.strip_suffix(".bz2") {
            let dir = TempDir::new().unwrap();
            let dec_path = dir.path().join(dec_name);
            std::io::copy(
                &mut File::open(&path).unwrap(),
                &mut bzip2::write::BzDecoder::new(File::create(&dec_path).unwrap()),
            )
            .unwrap();
            tracing::trace!("decompressed {} to {}", path.display(), dec_path.display());
            GuardedPath {
                path: dec_path,
                _guard: Some(dir),
            }
        } else {
            GuardedPath { path, _guard: None }
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        let gp = self.absolute_path();
        std::fs::read(gp.path).unwrap()
    }

    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }

    pub fn encoding(mut self, enc: Encoding) -> Self {
        self.expected_encoding = Some(enc);
        self
    }

    pub fn comment(mut self, comment: &'static str) -> Self {
        self.comment = Some(comment);
        self
    }

    pub fn files<F: Into<Files>>(mut self, files: F) -> Self {
        self.files = files.into();
        self
    }

    pub fn error<E: Into<Error>>(mut self, error: E) -> Self {
        self.error = Some(error.into());
        self
    }
}

pub struct CaseFile {
    pub name: &'static str,
    pub mode: Option<u32>,
    pub modified: Option<DateTime<Utc>>,
    pub content: FileContent,
}

impl CaseFile {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }

    pub fn mode(mut self, mode: u32) -> Self {
        self.mode = Some(mode);
        self
    }

    pub fn modified(mut self, date: DateTime<Utc>) -> Self {
        self.modified = Some(date);
        self
    }

    pub fn content<C: Into<FileContent>>(mut self, content: C) -> Self {
        self.content = content.into();
        self
    }
}

#[derive(Default)]
pub enum FileContent {
    #[default]
    Unchecked,
    Bytes(Vec<u8>),
    File(&'static str),
}

impl From<&str> for FileContent {
    fn from(s: &str) -> Self {
        s.as_bytes().into()
    }
}

impl From<&[u8]> for FileContent {
    fn from(bytes: &[u8]) -> Self {
        bytes.to_vec().into()
    }
}

impl From<String> for FileContent {
    fn from(s: String) -> Self {
        s.into_bytes().into()
    }
}

impl From<Vec<u8>> for FileContent {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Bytes(bytes)
    }
}

impl Default for CaseFile {
    fn default() -> Self {
        Self {
            name: "default",
            mode: None,
            modified: None,
            content: FileContent::default(),
        }
    }
}

pub fn zips_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("testdata")
}

fn time_zone(hours: i32) -> FixedOffset {
    FixedOffset::east_opt(hours * 3600).unwrap()
}

#[track_caller]
fn date(
    (year, month, day): (i32, u32, u32),
    (hour, min, sec): (u32, u32, u32),
    nsec: u32,
    offset: FixedOffset,
) -> DateTime<Utc> {
    offset
        .with_ymd_and_hms(year, month, day, hour, min, sec)
        .single()
        .unwrap()
        .with_nanosecond(nsec)
        .unwrap()
        .into()
}

pub fn test_cases() -> Vec<Case> {
    vec![
        Case::new("zip64.zip").files(
            CaseFile::new("README")
                .content("This small file is in ZIP64 format.\n")
                .modified(date((2012, 8, 10), (14, 33, 32), 0, time_zone(0)))
                .mode(0o644),
        ),
        Case::new("test.zip")
            .comment("This is a zipfile comment.")
            .encoding(Encoding::Utf8)
            .files(vec![
                CaseFile::new("test.txt")
                    .content("This is a test text file.\n")
                    .modified(date((2010, 9, 5), (12, 12, 1), 0, time_zone(10)))
                    .mode(0o644),
                CaseFile::new("gophercolor16x16.png")
                    .content(FileContent::File("gophercolor16x16.png"))
                    .modified(date((2010, 9, 5), (15, 52, 58), 0, time_zone(10)))
                    .mode(0o644),
            ]),
        Case::new("cp-437.zip")
            .encoding(Encoding::Cp437)
            .files(CaseFile::new("français")),
        Case::new("shift-jis.zip")
            .encoding(Encoding::ShiftJis)
            .files(vec![
                CaseFile::new("should-be-jis/"),
                CaseFile::new("should-be-jis/ot_運命のワルツﾈぞなぞ小さな楽しみ遊びま.longboi"),
            ]),
        Case::new("utf8-winrar.zip").encoding(Encoding::Utf8).files(
            CaseFile::new("世界").content("").modified(date(
                (2017, 11, 6),
                (21, 9, 27),
                867862500,
                time_zone(0),
            )),
        ),
        Case::new("meta.zip").files(33),
        Case::new("wine-zeroed.zip.bz2")
            .encoding(Encoding::Utf8)
            .files(11372),
        Case::new("info-zip-unix-extra.zip").files(CaseFile::new("bun-darwin-x64/")),
        Case::new("readme.trailingzip").files(CaseFile::new("README")),
        Case::new("archive-oob.zip").error(std::io::Error::other(
            "archive tried reading beyond zip archive end. 65536 goes beyond 42",
        )),
        Case::new("symlink.zip").files(vec![
            CaseFile::new("empty").content(""),
            CaseFile::new("symlink"),
        ]),
        #[cfg(feature = "lzma")]
        Case::new("found-me-lzma.zip")
            .encoding(Encoding::Utf8)
            .files(
                CaseFile::new("found-me.txt")
                    .content("Oh no, you found me\n".repeat(5000))
                    .modified(date((2024, 1, 26), (16, 14, 35), 46003100, time_zone(0))),
            ),
        #[cfg(feature = "deflate64")]
        Case::new("found-me-deflate64.zip")
            .encoding(Encoding::Utf8)
            .files(
                CaseFile::new("found-me.txt")
                    .content("Oh no, you found me\n".repeat(5000))
                    .modified(date((2024, 1, 26), (16, 14, 35), 46003100, time_zone(0))),
            ),
        // same with bzip2
        #[cfg(feature = "bzip2")]
        Case::new("found-me-bzip2.zip")
            .encoding(Encoding::Utf8)
            .files(
                CaseFile::new("found-me.txt")
                    .content("Oh no, you found me\n".repeat(5000))
                    .modified(date((2024, 1, 26), (16, 14, 35), 46003100, time_zone(0))),
            ),
        // same with zstd
        #[cfg(feature = "zstd")]
        Case::new("found-me-zstd.zip")
            .encoding(Encoding::Utf8)
            .files(
                CaseFile::new("found-me.txt")
                    .content("Oh no, you found me\n".repeat(5000))
                    .modified(date((2024, 1, 31), (6, 10, 25), 800491400, time_zone(0))),
            ),
    ]
}

pub fn streaming_test_cases() -> Vec<Case> {
    vec![
        Case::new("meta.zip").files(33),
        Case::new("info-zip-unix-extra.zip").files(CaseFile::new("bun-darwin-x64/")),
        Case::new("readme.trailingzip").error(FormatError::InvalidLocalHeader),
        Case::new("cp-437.zip").files(CaseFile::new("français")),
    ]
}

pub fn check_case(case: &Case, archive: Result<&Archive, &Error>) {
    let case_bytes = case.bytes();

    if let Some(expected) = &case.error {
        let actual = match archive {
            Err(e) => e,
            Ok(_) => panic!("should have failed"),
        };
        let expected = format!("{:#?}", expected);
        let actual = format!("{:#?}", actual);
        assert_eq!(expected, actual);
        return;
    }
    let archive = archive.unwrap_or_else(|e| {
        panic!(
            "{} should have succeeded, but instead: {e:?} ({e})",
            case.name
        )
    });

    assert_eq!(case_bytes.len() as u64, archive.size());

    if let Some(expected) = case.comment {
        assert_eq!(expected, archive.comment())
    }

    if let Some(exp_encoding) = case.expected_encoding {
        assert_eq!(archive.encoding(), exp_encoding);
    }

    assert_eq!(
        case.files.len(),
        archive.entries().count(),
        "{} should have {} entries files",
        case.name,
        case.files.len()
    );

    // then each implementation should check individual files
}

pub fn check_file_against(file: &CaseFile, entry: &Entry, actual_bytes: &[u8]) {
    if let Some(expected) = file.modified {
        assert_eq!(
            expected, entry.modified,
            "entry {} should have modified = {:?}",
            entry.name, expected
        )
    }

    if let Some(mode) = file.mode {
        assert_eq!(entry.mode.0 & 0o777, mode);
    }

    // I have honestly yet to see a zip file _entry_ with a comment.
    assert!(entry.comment.is_empty());

    match entry.kind() {
        EntryKind::File => {
            match &file.content {
                FileContent::Unchecked => {
                    // ah well
                }
                FileContent::Bytes(expected_bytes) => {
                    // first check length
                    assert_eq!(actual_bytes.len(), expected_bytes.len());
                    assert_eq!(actual_bytes, &expected_bytes[..])
                }
                FileContent::File(file_path) => {
                    let expected_bytes = std::fs::read(zips_dir().join(file_path)).unwrap();
                    // first check length
                    assert_eq!(actual_bytes.len(), expected_bytes.len());
                    assert_eq!(actual_bytes, &expected_bytes[..])
                }
            }
        }
        EntryKind::Symlink | EntryKind::Directory => {
            assert!(matches!(file.content, FileContent::Unchecked));
        }
    }
}

// This test subscriber is used to suppress trace-level logs (yet executes
// the code, for coverage reasons)
pub fn install_test_subscriber() {
    let env_filter = tracing_subscriber::EnvFilter::builder().from_env_lossy();
    let sub = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(env_filter)
        .with_test_writer()
        .finish();
    let sub = DebugOnlySubscriber { inner: sub };
    // fails when called multiple times from the same process (like in `cargo test`), so ignore
    // errors
    let _ = tracing::subscriber::set_global_default(sub);
}

struct DebugOnlySubscriber<S> {
    inner: S,
}

impl<S> tracing::Subscriber for DebugOnlySubscriber<S>
where
    S: tracing::Subscriber,
{
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, span: &span::Attributes<'_>) -> span::Id {
        self.inner.new_span(span)
    }

    fn record(&self, span: &span::Id, values: &span::Record<'_>) {
        self.inner.record(span, values)
    }

    fn record_follows_from(&self, span: &span::Id, follows: &span::Id) {
        self.inner.record_follows_from(span, follows)
    }

    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() == tracing::Level::TRACE {
            return;
        }

        self.inner.event(event)
    }

    fn enter(&self, span: &span::Id) {
        self.inner.enter(span)
    }

    fn exit(&self, span: &span::Id) {
        self.inner.exit(span)
    }
}
