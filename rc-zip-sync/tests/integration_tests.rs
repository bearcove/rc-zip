use rc_zip::{
    corpus::{self, zips_dir, FileContent, ZipTest, ZipTestFile},
    error::Error,
    parse::{Archive, EntryContents},
};
use rc_zip_sync::{HasCursor, ReadZip, SyncArchive, SyncStoredEntry};

use std::{cmp, fs::File};

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

    let archive_inner: &Archive = archive;
    let entry_inner = archive_inner.by_name(file.name).unwrap();
    assert_eq!(entry.name(), entry_inner.name());

    check_file_against(file, entry)
}

fn check_file_against<F: HasCursor>(file: &ZipTestFile, entry: SyncStoredEntry<'_, F>) {
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
            let actual_bytes = entry.bytes().unwrap();

            match &file.content {
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
        EntryContents::Symlink | EntryContents::Directory => {
            assert!(matches!(file.content, FileContent::Unchecked));
        }
    }
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

#[test_log::test]
fn state_machine() {
    use rc_zip::fsm::{ArchiveFsm, FsmResult};

    let cases = corpus::test_cases();
    let case = cases.iter().find(|x| x.name() == "zip64.zip").unwrap();
    let bs = case.bytes();
    let mut fsm = ArchiveFsm::new(bs.len() as u64);

    let archive = 'read_zip: loop {
        if let Some(offset) = fsm.wants_read() {
            let increment = 128usize;
            let offset = offset as usize;
            let slice = if offset + increment > bs.len() {
                &bs[offset..]
            } else {
                &bs[offset..offset + increment]
            };

            let len = cmp::min(slice.len(), fsm.space().len());
            fsm.space()[..len].copy_from_slice(&slice[..len]);
            match len {
                0 => panic!("EOF!"),
                read_bytes => {
                    fsm.fill(read_bytes);
                }
            }
        }

        fsm = match fsm.process() {
            Ok(res) => match res {
                FsmResult::Continue(fsm) => fsm,
                FsmResult::Done(archive) => break 'read_zip archive,
            },
            Err(err) => {
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
