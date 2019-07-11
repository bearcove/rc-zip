#![allow(clippy::all)]

mod encoding;
mod error;
mod reader;
mod types;

pub use positioned_io;
pub use reader::ZipReader;

#[cfg(test)]
mod tests {
    use super::{encoding::Encoding, ZipReader};
    use std::path::PathBuf;

    struct TestCase {
        name: &'static str,

        expected_encoding: Option<Encoding>,
    }

    impl TestCase {
        fn path(&self) -> PathBuf {
            let zips_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("test-zips");
            zips_dir.join(self.name)
        }

        fn bytes(&self) -> Vec<u8> {
            std::fs::read(self.path()).unwrap()
        }

        fn zip_reader(&self) -> ZipReader {
            let contents = self.bytes();
            ZipReader::new(&contents, contents.len() as u64).unwrap()
        }
    }

    fn test_cases() -> Vec<TestCase> {
        vec![
            TestCase {
                name: "cp-437.zip",
                expected_encoding: Some(Encoding::Cp437),
            },
            TestCase {
                name: "crc32-not-streamed.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "dd.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "go-no-datadesc-sig.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "go-with-datadesc-sig.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "readme.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "shift-jis.zip",
                expected_encoding: Some(Encoding::ShiftJis),
            },
            TestCase {
                name: "symlink.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "test.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "test-trailing-junk.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "time-22738.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "time-7zip.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "time-go.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "time-infozip.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "time-osx.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "time-win7.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "time-winrar.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "time-winzip.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "unix.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "utf8-7zip.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "utf8-infozip.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "utf8-osx.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "utf8-winrar.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "utf8-winzip.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "zip64.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
            TestCase {
                name: "zip64-2.zip",
                expected_encoding: Some(Encoding::Utf8),
            },
        ]
    }

    #[test]
    fn detect_encodings() {
        color_backtrace::install();

        for case in test_cases() {
            if let Some(encoding) = case.expected_encoding {
                println!("{}: should be {}", case.name, encoding);
                let reader = case.zip_reader();
                assert_eq!(reader.encoding(), encoding);
            }
        }
    }
}
