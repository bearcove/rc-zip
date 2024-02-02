#![allow(missing_docs)]

//! A corpus of zip files for testing.

use std::path::PathBuf;

use chrono::{DateTime, FixedOffset, TimeZone, Timelike, Utc};

use crate::{
    encoding::Encoding,
    error::Error,
    parse::{Archive, EntryContents, StoredEntry},
};

pub struct Case {
    pub name: &'static str,
    pub expected_encoding: Option<Encoding>,
    pub comment: Option<&'static str>,
    pub files: Vec<CaseFile>,
    pub error: Option<Error>,
}

impl Default for Case {
    fn default() -> Self {
        Self {
            name: "test.zip",
            expected_encoding: None,
            comment: None,
            files: vec![],
            error: None,
        }
    }
}

impl Case {
    pub fn absolute_path(&self) -> PathBuf {
        zips_dir().join(self.name)
    }
}

pub struct CaseFile {
    pub name: &'static str,
    pub mode: Option<u32>,
    pub modified: Option<DateTime<Utc>>,
    pub content: FileContent,
}

pub enum FileContent {
    Unchecked,
    Bytes(Vec<u8>),
    File(&'static str),
}

impl Default for CaseFile {
    fn default() -> Self {
        Self {
            name: "default",
            mode: None,
            modified: None,
            content: FileContent::Unchecked,
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

pub fn test_cases() -> Vec<Case> {
    vec![
        Case {
            name: "zip64.zip",
            files: vec![CaseFile {
                name: "README",
                content: FileContent::Bytes(
                    "This small file is in ZIP64 format.\n".as_bytes().into(),
                ),
                modified: Some(date((2012, 8, 10), (14, 33, 32), 0, time_zone(0)).unwrap()),
                mode: Some(0o644),
            }],
            ..Default::default()
        },
        Case {
            name: "test.zip",
            comment: Some("This is a zipfile comment."),
            expected_encoding: Some(Encoding::Utf8),
            files: vec![
                CaseFile {
                    name: "test.txt",
                    content: FileContent::Bytes("This is a test text file.\n".as_bytes().into()),
                    modified: Some(date((2010, 9, 5), (12, 12, 1), 0, time_zone(10)).unwrap()),
                    mode: Some(0o644),
                },
                CaseFile {
                    name: "gophercolor16x16.png",
                    content: FileContent::File("gophercolor16x16.png"),
                    modified: Some(date((2010, 9, 5), (15, 52, 58), 0, time_zone(10)).unwrap()),
                    mode: Some(0o644),
                },
            ],
            ..Default::default()
        },
        Case {
            name: "cp-437.zip",
            expected_encoding: Some(Encoding::Cp437),
            files: vec![CaseFile {
                name: "français",
                ..Default::default()
            }],
            ..Default::default()
        },
        Case {
            name: "shift-jis.zip",
            expected_encoding: Some(Encoding::ShiftJis),
            files: vec![
                CaseFile {
                    name: "should-be-jis/",
                    ..Default::default()
                },
                CaseFile {
                    name: "should-be-jis/ot_運命のワルツﾈぞなぞ小さな楽しみ遊びま.longboi",
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
        Case {
            name: "utf8-winrar.zip",
            expected_encoding: Some(Encoding::Utf8),
            files: vec![CaseFile {
                name: "世界",
                content: FileContent::Bytes(vec![]),
                modified: Some(date((2017, 11, 6), (21, 9, 27), 867862500, time_zone(0)).unwrap()),
                ..Default::default()
            }],
            ..Default::default()
        },
        #[cfg(feature = "lzma")]
        Case {
            source: "found-me-lzma.zip",
            expected_encoding: Some(Encoding::Utf8),
            files: vec![CaseFile {
                name: "found-me.txt",
                content: FileContent::Bytes("Oh no, you found me\n".repeat(5000).into()),
                modified: Some(date((2024, 1, 26), (16, 14, 35), 46003100, time_zone(0)).unwrap()),
                ..Default::default()
            }],
            ..Default::default()
        },
        #[cfg(feature = "deflate64")]
        Case {
            source: "found-me-deflate64.zip",
            expected_encoding: Some(Encoding::Utf8),
            files: vec![CaseFile {
                name: "found-me.txt",
                content: FileContent::Bytes("Oh no, you found me\n".repeat(5000).into()),
                modified: Some(date((2024, 1, 26), (16, 14, 35), 46003100, time_zone(0)).unwrap()),
                ..Default::default()
            }],
            ..Default::default()
        },
        // same with bzip2
        #[cfg(feature = "bzip2")]
        Case {
            source: "found-me-bzip2.zip",
            expected_encoding: Some(Encoding::Utf8),
            files: vec![CaseFile {
                name: "found-me.txt",
                content: FileContent::Bytes("Oh no, you found me\n".repeat(5000).into()),
                modified: Some(date((2024, 1, 26), (16, 14, 35), 46003100, time_zone(0)).unwrap()),
                ..Default::default()
            }],
            ..Default::default()
        },
        // same with zstd
        #[cfg(feature = "zstd")]
        Case {
            source: "found-me-zstd.zip",
            expected_encoding: Some(Encoding::Utf8),
            files: vec![CaseFile {
                name: "found-me.txt",
                content: FileContent::Bytes("Oh no, you found me\n".repeat(5000).into()),
                modified: Some(date((2024, 1, 31), (6, 10, 25), 800491400, time_zone(0)).unwrap()),
                ..Default::default()
            }],
            ..Default::default()
        },
    ]
}

pub fn check_case(test: &Case, archive: Result<&Archive, &Error>) {
    let case_bytes = std::fs::read(test.absolute_path()).unwrap();

    if let Some(expected) = &test.error {
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

    if let Some(expected) = test.comment {
        assert_eq!(expected, archive.comment().expect("should have comment"))
    }

    if let Some(exp_encoding) = test.expected_encoding {
        assert_eq!(archive.encoding(), exp_encoding);
    }

    assert_eq!(
        test.files.len(),
        archive.entries().count(),
        "{} should have {} entries files",
        test.name,
        test.files.len()
    );

    // then each implementation should check individual files
}

pub fn check_file_against(file: &CaseFile, entry: &StoredEntry, actual_bytes: &[u8]) {
    if let Some(expected) = file.modified {
        assert_eq!(
            expected,
            entry.modified(),
            "entry {} should have modified = {:?}",
            entry.name(),
            expected
        )
    }

    if let Some(mode) = file.mode {
        assert_eq!(entry.mode.0 & 0o777, mode);
    }

    // I have honestly yet to see a zip file _entry_ with a comment.
    assert!(entry.comment().is_none());

    match entry.contents() {
        EntryContents::File => {
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
        EntryContents::Symlink | EntryContents::Directory => {
            assert!(matches!(file.content, FileContent::Unchecked));
        }
    }
}
