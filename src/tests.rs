use crate::reader::sync::EntryReader;

use super::{encoding::Encoding, prelude::*};
use chrono::{
    offset::{FixedOffset, Utc},
    DateTime, TimeZone,
};
use std::{io::Read, path::PathBuf};

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

#[derive(Debug)]
struct ZipTestFile {
    name: &'static str,
    mode: Option<u32>,
    modified: Option<DateTime<Utc>>,
    content: FileContent,
}

#[derive(Debug)]
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
        .join("resources")
        .join("test-zips")
}

fn time_zone(hours: i32) -> FixedOffset {
    FixedOffset::east(hours * 3600)
}

fn date(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
    nsec: u32,
    offset: FixedOffset,
) -> DateTime<Utc> {
    offset
        .ymd(year, month, day)
        .and_hms_nano(hour, min, sec, nsec)
        .into()
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
                modified: Some(date(2012, 8, 10, 14, 33, 32, 0, time_zone(0))),
                mode: Some(0o644),
                ..Default::default()
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
                    modified: Some(date(2010, 9, 5, 12, 12, 1, 0, time_zone(10))),
                    mode: Some(0o644),
                    ..Default::default()
                },
                ZipTestFile {
                    name: "gophercolor16x16.png",
                    content: FileContent::File("gophercolor16x16.png"),
                    modified: Some(date(2010, 9, 5, 15, 52, 58, 0, time_zone(10))),
                    mode: Some(0o644),
                    ..Default::default()
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
    ]
}

#[test]
fn real_world_files() {
    for case in test_cases() {
        let case_name = case.name();
        let case_bytes = case.bytes();
        let archive = case_bytes.read_zip();

        if let Some(expected) = case.error {
            let actual = archive.expect_err("should have errored");
            let expected = format!("{:#?}", expected);
            let actual = format!("{:#?}", actual);
            assert_eq!(expected, actual);
            continue;
        }
        let archive = archive.unwrap();

        if let Some(expected) = case.comment {
            assert_eq!(expected, archive.comment().expect("should have comment"))
        }

        if let Some(exp_encoding) = case.expected_encoding {
            println!("{}: should be {}", case.name(), exp_encoding);
            assert_eq!(archive.encoding(), exp_encoding);
        }

        assert_eq!(
            case.files.len(),
            archive.entries().len(),
            "{} should have {} entries files",
            case.name(),
            case.files.len()
        );

        for f in &case.files {
            let entry = archive
                .by_name(f.name)
                .expect("should have specific test file");

            if let Some(expected) = f.modified {
                assert_eq!(
                    expected,
                    entry.modified(),
                    "entry {} (in {}) should have modified = {:?}",
                    entry.name(),
                    case_name,
                    expected
                )
            }

            if let Some(mode) = f.mode {
                assert_eq!(entry.mode.0 & 0o777, mode);
            }

            match entry.contents() {
                crate::EntryContents::File(_) => {
                    let mut er = EntryReader::new(entry, |offset| {
                        positioned_io::Cursor::new_pos(case.bytes(), offset)
                    });
                    let mut actual_bytes = Vec::new();
                    er.read_to_end(&mut actual_bytes).unwrap();

                    match &f.content {
                        FileContent::Unchecked => {
                            // ah well
                        }
                        FileContent::Bytes(expected_bytes) => {
                            assert_eq!(&actual_bytes[..], &expected_bytes[..])
                        }
                        FileContent::File(file_path) => {
                            let expected_bytes = std::fs::read(zips_dir().join(file_path)).unwrap();
                            assert_eq!(&actual_bytes[..], &expected_bytes[..])
                        }
                    }
                }
                crate::EntryContents::Symlink(_) | crate::EntryContents::Directory(_) => {
                    assert!(matches!(f.content, FileContent::Unchecked));
                }
            }
        }
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
        match zar.wants_read() {
            Some(offset) => {
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
            None => {} // ok, cool, proceed,
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

    println!("All done! Archive = {:#?}", archive);
}
