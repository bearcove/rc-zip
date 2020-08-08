//! # rc-zip
//!
//! rc-zip is a zip archive library with a focus on compatibility and correctness.
//!
//! ### Reading
//!
//! [ArchiveReader](ArchiveReader) is your first stop. It
//! ensures we are dealing with a valid zip archive, and reads the central
//! directory. It does not perform I/O itself, but rather, it is a state machine
//! that asks for reads at specific offsets.
//!
//! An [Archive](Archive) contains a full list of [entries](types::StoredEntry),
//! which you can then extract.
//!
//! ### Writing
//!
//! Writing archives is not implemented yet.
//!
#![allow(clippy::all)]

mod encoding;
mod error;
mod format;
pub mod prelude;
mod reader;
mod writer;

pub use self::{error::*, format::*, reader::*, writer::*};

#[cfg(test)]
mod tests {
    use super::{encoding::Encoding, prelude::*, Archive, Error};
    use chrono::{
        offset::{FixedOffset, Utc},
        DateTime, TimeZone,
    };
    use std::path::PathBuf;

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
        Error(Error),
        Bytes(Vec<u8>),
        File(&'static str),
        Size(u64),
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

        fn bytes(&self) -> Vec<u8> {
            match &self.source {
                ZipSource::File(name) => {
                    let path = {
                        let zips_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .join("resources")
                            .join("test-zips");
                        zips_dir.join(name)
                    };
                    std::fs::read(path).unwrap()
                }
                ZipSource::Func(_name, f) => f(),
            }
        }

        fn archive(&self) -> Result<Archive, Error> {
            self.bytes().read_zip()
        }
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
                    mode: Some(0644),
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
                        content: FileContent::Bytes(
                            "This is a test text file.\n".as_bytes().into(),
                        ),
                        modified: Some(date(2010, 9, 5, 12, 12, 1, 0, time_zone(10))),
                        mode: Some(0644),
                        ..Default::default()
                    },
                    ZipTestFile {
                        name: "gophercolor16x16.png",
                        content: FileContent::File("gophercolor16x16.png"),
                        modified: Some(date(2010, 9, 5, 15, 52, 58, 0, time_zone(10))),
                        mode: Some(0644),
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
    fn detect_encodings() {
        color_backtrace::install();

        for case in test_cases() {
            if let Some(encoding) = case.expected_encoding {
                println!("{}: should be {}", case.name(), encoding);
                let archive = case.archive().unwrap();
                assert_eq!(archive.encoding(), encoding);
            }
        }
    }

    #[test]
    fn test_reader() {
        color_backtrace::install();

        for case in test_cases() {
            let case_name = case.name();
            let archive = case.archive();
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

            assert_eq!(
                case.files.len(),
                archive.entries().len(),
                "{} should have {} entries files",
                case.name(),
                case.files.len()
            );

            for f in case.files {
                let entry = archive
                    .by_name(f.name)
                    .expect("should have specific test file");

                {
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
                }
            }
        }
    }

    #[test]
    fn test_fsm() {
        use super::reader::{ArchiveReader, ArchiveReaderResult};
        env_logger::init();

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
                            panic!(err)
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
                    panic!(err)
                }
            }
        };

        println!("All done! Archive = {:#?}", archive);
    }
}
