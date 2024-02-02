use rc_zip::{
    corpus::{self, zips_dir, ZipTest, ZipTestFile},
    error::Error,
};
use rc_zip_sync::{HasCursor, ReadZip, SyncArchive};

use std::fs::File;

fn check_case<F: HasCursor>(test: &ZipTest, archive: Result<SyncArchive<'_, F>, Error>) {
    let case_bytes = test.bytes();

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
        test.name(),
        test.files.len()
    );

    for f in &test.files {
        check_file(f, &archive);
    }
}

fn check_file<F: HasCursor>(file: &ZipTestFile, archive: &SyncArchive<'_, F>) {
    let entry = archive
        .by_name(file.name)
        .unwrap_or_else(|| panic!("entry {} should exist", file.name));

    corpus::check_file_against(file, &entry, &entry.bytes().unwrap()[..])
}

#[test_log::test]
fn read_from_slice() {
    let bytes = std::fs::read(zips_dir().join("test.zip")).unwrap();
    let slice = &bytes[..];
    let archive = slice.read_zip().unwrap();
    assert_eq!(archive.entries().count(), 2);
}

#[test_log::test]
fn read_from_file() {
    let f = File::open(zips_dir().join("test.zip")).unwrap();
    let archive = f.read_zip().unwrap();
    assert_eq!(archive.entries().count(), 2);
}

#[test_log::test]
fn real_world_files() {
    for case in corpus::test_cases() {
        tracing::trace!("============ testing {}", case.name());
        check_case(&case, case.bytes().read_zip())
    }
}
