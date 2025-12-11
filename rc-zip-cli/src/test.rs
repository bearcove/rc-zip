use std::{fs, path::Path};

use crate::{unzip, unzip_streaming};

use rc_zip_corpus::{zips_dir, Case, CaseFile, FileContent, Files};
use rc_zip_sync::ReadZip;
use temp_dir::TempDir;
use walkdir::WalkDir;

fn check_case(
    case: &Case,
    unzip_fn: fn(&Path, Option<&Path>, bool) -> Result<(), Box<dyn std::error::Error>>,
) {
    let out_dir = TempDir::with_prefix("rc-zip-test-").unwrap();
    let out_path = out_dir.path();

    let guarded_path = case.absolute_path();
    let zip_path = &guarded_path.path;

    let hide_progress = true;
    let res = (unzip_fn)(zip_path, Some(out_path), hide_progress);
    match (res, &case.error) {
        (Ok(()), None) => { /* checked after */ }
        (Err(actual), Some(expected)) => {
            let expected = format!("{:#?}", expected);
            let actual = format!("{:#?}", actual);
            assert_eq!(expected, actual);
            return;
        }
        (Ok(()), Some(expected)) => panic!("succeeded, but should have failed with {expected:?}"),
        (Err(actual), None) => panic!("should have succeeded, but failed with {actual:?}"),
    }

    match &case.files {
        Files::NumFiles(expected) => {
            // `WalkDir` includes the directory itself, so `- 1` to account for it
            let actual = WalkDir::new(out_path).into_iter().count() - 1;
            assert_eq!(*expected, actual);
        }
        Files::ExhaustiveList(files) => {
            for file in files {
                let CaseFile {
                    name,
                    mode,
                    modified,
                    content,
                } = file;
                let extracted_path = out_path.join(name);

                match content {
                    FileContent::Unchecked => assert!(
                        extracted_path.exists(),
                        "expected {name} to exist, but it didn't"
                    ),
                    FileContent::Bytes(expected) => {
                        let actual = std::fs::read(&extracted_path).unwrap();
                        assert_eq!(expected.len(), actual.len());
                        assert_eq!(expected, &actual);
                    }
                    FileContent::File(expected_path) => {
                        let actual = std::fs::read(&extracted_path).unwrap();
                        let expected = std::fs::read(zips_dir().join(expected_path)).unwrap();
                        assert_eq!(expected.len(), actual.len());
                        assert_eq!(expected, actual);
                    }
                }

                if let Some(_expected) = mode {
                    // TODO(cosmic): platform specific behavior makes this non-trivial to test :S
                }

                if let Some(_expected) = modified {
                    // FIXME(cosmic): unsupported (for now)
                }
            }
        }
    }
}

#[test]
fn corpus() {
    rc_zip_corpus::install_test_subscriber();

    for case in rc_zip_corpus::test_cases() {
        tracing::info!("============ testing {}", case.name);
        check_case(&case, unzip);
    }
}

#[test]
fn corpus_streaming() {
    rc_zip_corpus::install_test_subscriber();

    for case in rc_zip_corpus::streaming_test_cases() {
        tracing::info!("============ testing {}", case.name);
        check_case(&case, unzip_streaming);
    }
}

#[test]
fn cli() {
    use clap::CommandFactory;
    crate::Cli::command().debug_assert();
}

#[test]
fn info() {
    #[track_caller]
    fn info_str(zip_name: &str) -> String {
        let zip_path = zips_dir().join(zip_name);
        let zip_file = fs::File::open(&zip_path).unwrap();
        let archive = zip_file.read_zip().unwrap();
        let mut output = Vec::new();
        crate::info(&mut output, &archive).unwrap();
        String::from_utf8(output).unwrap()
    }

    insta::assert_snapshot!(info_str("unix.zip"), @r"
    Versions: {MsDos v10}
    Encoding: utf-8, Methods: {Store}
    26 B (100.00% compression) (3 files, 1 dirs, 0 symlinks)
    ");
    insta::assert_snapshot!(info_str("meta.zip"), @r"
    Versions: {Unix v20}
    Encoding: utf-8, Methods: {Deflate}
    138.16 KiB (28.68% compression) (26 files, 7 dirs, 0 symlinks)
    ");
}

#[test]
fn list() {
    #[track_caller]
    fn list_str(zip_name: &str, verbose: bool) -> String {
        let zip_path = zips_dir().join(zip_name);
        let zip_file = fs::File::open(&zip_path).unwrap();
        let archive = zip_file.read_zip().unwrap();
        let mut output = Vec::new();
        crate::list(&mut output, &archive, verbose).unwrap();
        String::from_utf8(output).unwrap()
    }

    insta::assert_snapshot!(list_str("symlink.zip", true), @r"
    -rw-r--r--          0 B empty (0 B compressed) 2025-12-11 03:59:41 UTC 1000 1000	Store
    -rw-r--r--          0 B symlink (0 B compressed) 2025-12-11 03:59:41 UTC 1000 1000	Store
    ");
    insta::assert_snapshot!(list_str("utf8-infozip.zip", false), @"-rw-r--r--          0 B 世界");
    insta::assert_snapshot!(list_str("unix.zip", false), @r"
    -rw-rw-rw-          8 B hello
    -rw-rw-rw-          6 B dir/bar
    drwxrwxrwx          0 B dir/empty/
    -r--r--r--         12 B readonly
    ");
}
