#![allow(clippy::all)]

mod encoding;
mod error;
mod reader;
mod types;

pub use error::Error;
pub use positioned_io;
pub use reader::ZipReader;

#[cfg(test)]
mod tests {
    use super::{encoding::Encoding, Error, ZipReader};
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

        fn zip_reader(&self) -> Result<ZipReader, Error> {
            let contents = self.bytes();
            ZipReader::new(&contents, contents.len() as u64)
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
                source: ZipSource::File("test.zip"),
                comment: Some("This is a zipfile comment."),
                expected_encoding: Some(Encoding::Utf8),
                files: vec![
                    ZipTestFile {
                        name: "test.txt",
                        content: FileContent::Bytes(
                            "This is a test text file.\n".as_bytes().into(),
                        ),
                        modified: Some(date(2019, 9, 5, 12, 12, 1, 0, time_zone(10))),
                        mode: Some(0644),
                        ..Default::default()
                    },
                    ZipTestFile {
                        name: "gophercolor16x16.png",
                        content: FileContent::File("gophercolor16x16.png"),
                        modified: Some(date(2019, 9, 5, 12, 52, 58, 0, time_zone(10))),
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
                let reader = case.zip_reader().unwrap();
                assert_eq!(reader.encoding(), encoding);
            }
        }
    }

    #[test]
    fn test_reader() {
        color_backtrace::install();

        for case in test_cases() {
            let reader = case.zip_reader();
            if let Some(expected) = case.error {
                let actual = reader.expect_err("should have errored");
                let expected = format!("{:#?}", expected);
                let actual = format!("{:#?}", actual);
                assert_eq!(expected, actual);
                continue;
            }
            let reader = reader.unwrap();
            if let Some(expected) = case.comment {
                assert_eq!(expected, reader.comment().expect("should have comment"))
            }

            assert_eq!(
                case.files.len(),
                reader.entries().len(),
                "{} should have {} entries files",
                case.name(),
                case.files.len()
            );

            for f in case.files {
                let entry = reader
                    .by_name(f.name)
                    .expect("should have specific test file");
                println!("test entry: {:#?}", entry);
            }
        }
    }
}
