use positioned_io::{RandomAccessFile, Size};
use rc_zip::{error::Error, parse::Archive};
use rc_zip_corpus::{zips_dir, Case, Files};
use rc_zip_tokio::{ArchiveHandle, HasCursor, ReadZip, ReadZipStreaming, ReadZipWithSize};
use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};

use std::{pin::Pin, sync::Arc, task};

async fn check_case<F: HasCursor>(test: &Case, archive: Result<ArchiveHandle<'_, F>, Error>) {
    rc_zip_corpus::check_case(test, archive.as_ref().map(|ar| -> &Archive { ar }));
    let archive = match archive {
        Ok(archive) => archive,
        Err(_) => return,
    };

    if let Files::ExhaustiveList(files) = &test.files {
        for file in files {
            let entry = archive
                .by_name(file.name)
                .unwrap_or_else(|| panic!("entry {} should exist", file.name));

            rc_zip_corpus::check_file_against(file, &entry, &entry.bytes().await.unwrap()[..])
        }
    }
}

#[tokio::test]
async fn read_from_slice() {
    rc_zip_corpus::install_test_subscriber();

    let bytes = std::fs::read(zips_dir().join("test.zip")).unwrap();
    let slice = &bytes[..];
    let archive = slice.read_zip().await.unwrap();
    assert_eq!(archive.entries().count(), 2);
}

#[tokio::test]
async fn read_from_file() {
    rc_zip_corpus::install_test_subscriber();

    let f = Arc::new(RandomAccessFile::open(zips_dir().join("test.zip")).unwrap());
    let archive = f.read_zip().await.unwrap();
    assert_eq!(archive.entries().count(), 2);
}

#[tokio::test]
async fn real_world_files() {
    rc_zip_corpus::install_test_subscriber();

    for case in rc_zip_corpus::test_cases() {
        tracing::info!("============ testing {}", case.name);

        let guarded_path = case.absolute_path();
        let file = Arc::new(RandomAccessFile::open(&guarded_path.path).unwrap());
        if let Ok("1") = std::env::var("ONE_BYTE_READ").as_deref() {
            let size = file.size().unwrap().expect("file to have a size");
            let file = OneByteReadWrapper(file);
            let archive = file.read_zip_with_size(size).await;
            check_case(&case, archive).await;
        } else {
            let archive = file.read_zip().await;
            check_case(&case, archive).await;
        }
        drop(guarded_path)
    }
}

#[tokio::test]
async fn streaming() {
    rc_zip_corpus::install_test_subscriber();

    for case in rc_zip_corpus::streaming_test_cases() {
        let guarded_path = case.absolute_path();
        let file = tokio::fs::File::open(&guarded_path.path).await.unwrap();

        let mut entry = match file.stream_zip_entries_throwing_caution_to_the_wind().await {
            Ok(entry) => entry,
            Err(err) => {
                check_case::<&[u8]>(&case, Err(err)).await;
                return;
            }
        };
        loop {
            let mut v = vec![];
            let n = entry.read_to_end(&mut v).await.unwrap();
            tracing::trace!("entry {} read {} bytes", entry.entry().name, n);

            match entry.finish().await.unwrap() {
                Some(next) => entry = next,
                None => break,
            }
        }

        drop(guarded_path)
    }
}

// This helps find bugs in state machines!

struct OneByteReadWrapper<R>(R);

impl<R> AsyncRead for OneByteReadWrapper<R>
where
    R: AsyncRead,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> task::Poll<std::io::Result<()>> {
        let mut inner_buf = buf.take(1);
        futures_util::ready!(
            unsafe { self.map_unchecked_mut(|s| &mut s.0) }.poll_read(cx, &mut inner_buf)
        )?;
        let n = inner_buf.filled().len();
        buf.set_filled(n);
        Ok(()).into()
    }
}

impl<R> HasCursor for OneByteReadWrapper<R>
where
    R: HasCursor,
{
    type Cursor<'a>
        = OneByteReadWrapper<<R as HasCursor>::Cursor<'a>>
    where
        R: 'a;

    fn cursor_at(&self, offset: u64) -> Self::Cursor<'_> {
        OneByteReadWrapper(self.0.cursor_at(offset))
    }
}
