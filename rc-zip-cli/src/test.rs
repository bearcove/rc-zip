use std::path::Path;

use crate::{unzip, unzip_streaming};

use rc_zip_corpus::{zips_dir, Case, CaseFile, FileContent, Files};
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
