use crate::StoredEntry;
use ara::ReadAt;
use futures::io::AsyncRead;

pub struct AsyncEntryReader<'a, R>
where
    R: ReadAt,
{
    entry: &'a StoredEntry,
    source: R,
}

impl<'a, R> AsyncEntryReader<'a, R>
where
    R: ReadAt,
{
    pub fn new(entry: &'a StoredEntry, source: R) -> Self {
        Self { entry, source }
    }
}

impl<'a, R> AsyncRead for AsyncEntryReader<'a, R>
where
    R: ReadAt,
{
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        todo!()
    }
}
