use crate::{
    reader::sync::{HasCursor, SyncArchive, SyncStoredEntry},
    Archive,
};

use super::{encoding::Encoding, prelude::*};
use chrono::{
    offset::{FixedOffset, Utc},
    DateTime, TimeZone, Timelike,
};
use std::{fs::File, path::PathBuf};

enum ZipSource {
    File(&'static str),
    Func(&'static str, Box<dyn Fn() -> Vec<u8>>),
}

struct ZipTest {
    source: ZipSource,
    expected_encoding: Option<Encoding>,
    comment: Option<&'static str>,
    files: Vec<ZipTestFile>,
    error: Option<super::Error>,
}

impl Default for ZipTest {
    fn default() -> Self {
        Self {
            source: ZipSource::Func("default.zip", Box::new(|| unreachable!())),
            expected_encoding: None,
            comment: None,
            files: vec![],
            error: None,
        }
    }
}

impl ZipTest {
    fn check<F: HasCursor>(&self, archive: Result<SyncArchive<'_, F>, crate::Error>) {
        let case_bytes = self.bytes();

        if let Some(expected) = &self.error {
            let actual = match archive {
                Err(e) => e,
                Ok(_) => panic!("should have failed"),
            };
            let expected = format!("{:#?}", expected);
            let actual = format!("{:#?}", actual);
            assert_eq!(expected, actual);
            return;
        }
        let archive = archive.unwrap();

        assert_eq!(case_bytes.len() as u64, archive.size());

        if let Some(expected) = self.comment {
            assert_eq!(expected, archive.comment().expect("should have comment"))
        }

        if let Some(exp_encoding) = self.expected_encoding {
            println!("{}: should be {}", self.name(), exp_encoding);
            assert_eq!(archive.encoding(), exp_encoding);
        }

        assert_eq!(
            self.files.len(),
            archive.entries().count(),
            "{} should have {} entries files",
            self.name(),
            self.files.len()
        );

        for f in &self.files {
            f.check(&archive);
        }
    }
}

struct ZipTestFile {
    name: &'static str,
    mode: Option<u32>,
    modified: Option<DateTime<Utc>>,
    content: FileContent,
}

impl ZipTestFile {
    fn check<F: HasCursor>(&self, archive: &SyncArchive<'_, F>) {
        let entry = archive
            .by_name(self.name)
            .unwrap_or_else(|| panic!("entry {} should exist", self.name));

        let archive_inner: &Archive = archive;
        let entry_inner = archive_inner.by_name(self.name).unwrap();
        assert_eq!(entry.name(), entry_inner.name());

        self.check_against(entry);
    }

    fn check_against<F: HasCursor>(&self, entry: SyncStoredEntry<'_, F>) {
        if let Some(expected) = self.modified {
            assert_eq!(
                expected,
                entry.modified(),
                "entry {} should have modified = {:?}",
                entry.name(),
                expected
            )
        }

        if let Some(mode) = self.mode {
            assert_eq!(entry.mode.0 & 0o777, mode);
        }

        // I have honestly yet to see a zip file _entry_ with a comment.
        assert!(entry.comment().is_none());

        match entry.contents() {
            crate::EntryContents::File => {
                let actual_bytes = entry.bytes().unwrap();

                match &self.content {
                    FileContent::Unchecked => {
                        // ah well
                    }
                    FileContent::Bytes(expected_bytes) => {
                        // first check length
                        assert_eq!(actual_bytes.len(), expected_bytes.len());
                        assert_eq!(&actual_bytes[..], &expected_bytes[..])
                    }
                    FileContent::File(file_path) => {
                        let expected_bytes = std::fs::read(zips_dir().join(file_path)).unwrap();
                        // first check length
                        assert_eq!(actual_bytes.len(), expected_bytes.len());
                        assert_eq!(&actual_bytes[..], &expected_bytes[..])
                    }
                }
            }
            crate::EntryContents::Symlink | crate::EntryContents::Directory => {
                assert!(matches!(self.content, FileContent::Unchecked));
            }
        }
    }
}

enum FileContent {
    Unchecked,
    Bytes(Vec<u8>),
    File(&'static str),
}

impl Default for ZipTestFile {
    fn default() -> Self {
        Self {
            name: "default",
            mode: None,
            modified: None,
            content: FileContent::Unchecked,
        }
    }
}

impl ZipTest {
    fn name(&self) -> &'static str {
        match &self.source {
            ZipSource::File(name) => name,
            ZipSource::Func(name, _f) => name,
        }
    }

    // Read source archive from disk
    fn bytes(&self) -> Vec<u8> {
        match &self.source {
            ZipSource::File(name) => std::fs::read(zips_dir().join(name)).unwrap(),
            ZipSource::Func(_name, f) => f(),
        }
    }
}

fn zips_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("test-zips")
}

fn time_zone(hours: i32) -> FixedOffset {
    FixedOffset::east_opt(hours * 3600).unwrap()
}

fn date(
    (year, month, day): (i32, u32, u32),
    (hour, min, sec): (u32, u32, u32),
    nsec: u32,
    offset: FixedOffset,
) -> Option<DateTime<Utc>> {
    Some(
        offset
            .with_ymd_and_hms(year, month, day, hour, min, sec)
            .single()?
            .with_nanosecond(nsec)?
            .into(),
    )
}

fn test_cases() -> Vec<ZipTest> {
    vec![
        ZipTest {
            source: ZipSource::File("zip64.zip"),
            files: vec![ZipTestFile {
                name: "README",
                content: FileContent::Bytes(
                    "This small file is in ZIP64 format.\n".as_bytes().into(),
                ),
                modified: Some(date((2012, 8, 10), (14, 33, 32), 0, time_zone(0)).unwrap()),
                mode: Some(0o644),
            }],
            ..Default::default()
        },
        ZipTest {
            source: ZipSource::File("test.zip"),
            comment: Some("This is a zipfile comment."),
            expected_encoding: Some(Encoding::Utf8),
            files: vec![
                ZipTestFile {
                    name: "test.txt",
                    content: FileContent::Bytes("This is a test text file.\n".as_bytes().into()),
                    modified: Some(date((2010, 9, 5), (12, 12, 1), 0, time_zone(10)).unwrap()),
                    mode: Some(0o644),
                },
                ZipTestFile {
                    name: "gophercolor16x16.png",
                    content: FileContent::File("gophercolor16x16.png"),
                    modified: Some(date((2010, 9, 5), (15, 52, 58), 0, time_zone(10)).unwrap()),
                    mode: Some(0o644),
                },
            ],
            ..Default::default()
        },
        ZipTest {
            source: ZipSource::File("cp-437.zip"),
            expected_encoding: Some(Encoding::Cp437),
            files: vec![ZipTestFile {
                name: "français",
                ..Default::default()
            }],
            ..Default::default()
        },
        ZipTest {
            source: ZipSource::File("shift-jis.zip"),
            expected_encoding: Some(Encoding::ShiftJis),
            files: vec![
                ZipTestFile {
                    name: "should-be-jis/",
                    ..Default::default()
                },
                ZipTestFile {
                    name: "should-be-jis/ot_運命のワルツﾈぞなぞ小さな楽しみ遊びま.longboi",
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
        ZipTest {
            source: ZipSource::File("utf8-winrar.zip"),
            expected_encoding: Some(Encoding::Utf8),
            files: vec![ZipTestFile {
                name: "世界",
                content: FileContent::Bytes(vec![]),
                modified: Some(date((2017, 11, 6), (13, 9, 26), 0, time_zone(0)).unwrap()),
                ..Default::default()
            }],
            ..Default::default()
        },
        #[cfg(feature = "lzma")]
        ZipTest {
            source: ZipSource::File("found-me-lzma.zip"),
            expected_encoding: Some(Encoding::Utf8),
            files: vec![ZipTestFile {
                name: "found-me.txt",
                content: FileContent::Bytes("Oh no, you found me\n".repeat(5000).into()),
                modified: Some(date((2024, 1, 26), (17, 14, 36), 0, time_zone(0)).unwrap()),
                ..Default::default()
            }],
            ..Default::default()
        },
    ]
}

#[test]
fn read_from_slice() {
    let bytes = std::fs::read(zips_dir().join("test.zip")).unwrap();
    let slice = &bytes[..];
    let archive = slice.read_zip().unwrap();
    assert_eq!(archive.entries().count(), 2);
}

#[test]
fn read_from_file() {
    let f = File::open(zips_dir().join("test.zip")).unwrap();
    let archive = f.read_zip().unwrap();
    assert_eq!(archive.entries().count(), 2);
}

#[test]
fn real_world_files() {
    for case in test_cases() {
        case.check(case.bytes().read_zip());
    }
}

#[test]
fn test_fsm() {
    use super::reader::{ArchiveReader, ArchiveReaderResult};

    let cases = test_cases();
    let case = cases.iter().find(|x| x.name() == "zip64.zip").unwrap();
    let bs = case.bytes();
    let mut zar = ArchiveReader::new(bs.len() as u64);

    let archive = 'read_zip: loop {
        if let Some(offset) = zar.wants_read() {
            let increment = 128usize;
            let offset = offset as usize;
            let mut slice = if offset + increment > bs.len() {
                &bs[offset..]
            } else {
                &bs[offset..offset + increment]
            };

            match zar.read(&mut slice) {
                Ok(0) => panic!("EOF!"),
                Ok(read_bytes) => {
                    println!("at {}, zar read {} bytes", offset, read_bytes);
                }
                Err(err) => {
                    println!("at {}, zar encountered an error:", offset);
                    panic!("{}", err)
                }
            }
        }

        match zar.process() {
            Ok(res) => match res {
                ArchiveReaderResult::Continue => {}
                ArchiveReaderResult::Done(archive) => break 'read_zip archive,
            },
            Err(err) => {
                println!("zar processing error: {:#?}", err);
                panic!("{}", err)
            }
        }
    };

    let sync_archive = bs.read_zip().unwrap();
    for (se, e) in sync_archive.entries().zip(archive.entries()) {
        assert_eq!(se.name(), e.name());
        assert_eq!(se.inner.compressed_size, e.inner.compressed_size);
        assert_eq!(se.inner.uncompressed_size, e.inner.uncompressed_size);
    }
}
