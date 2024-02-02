use positioned_io::RandomAccessFile;
use rc_zip::{
    corpus::{self, zips_dir, Case},
    error::Error,
    parse::Archive,
};
use rc_zip_tokio::{AsyncArchive, HasAsyncCursor, ReadZipAsync};

use std::sync::Arc;

async fn check_case<F: HasAsyncCursor>(test: &Case, archive: Result<AsyncArchive<'_, F>, Error>) {
    corpus::check_case(test, archive.as_ref().map(|ar| -> &Archive { ar }));
    let archive = match archive {
        Ok(archive) => archive,
        Err(_) => return,
    };

    for file in &test.files {
        let entry = archive
            .by_name(file.name)
            .unwrap_or_else(|| panic!("entry {} should exist", file.name));

        corpus::check_file_against(file, &entry, &entry.bytes().await.unwrap()[..])
    }
}

#[test_log::test(tokio::test)]
async fn read_from_slice() {
    let bytes = std::fs::read(zips_dir().join("test.zip")).unwrap();
    let slice = &bytes[..];
    let archive = slice.read_zip_async().await.unwrap();
    assert_eq!(archive.entries().count(), 2);
}

#[test_log::test(tokio::test)]
async fn read_from_file() {
    let f = Arc::new(RandomAccessFile::open(zips_dir().join("test.zip")).unwrap());
    let archive = f.read_zip_async().await.unwrap();
    assert_eq!(archive.entries().count(), 2);
}

#[test_log::test(tokio::test)]
async fn real_world_files() {
    for case in corpus::test_cases() {
        tracing::info!("============ testing {}", case.name);

        let file = Arc::new(RandomAccessFile::open(case.absolute_path()).unwrap());
        let archive = file.read_zip_async().await;

        check_case(&case, archive).await
    }
}
