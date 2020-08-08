use crate::StoredEntry;
use ara::ReadAt;
use futures::{
    io::{self, AsyncRead},
    Future,
};
use std::{
    pin::Pin,
    task::{Context, Poll},
};

struct Inner<R>
where
    R: ReadAt,
{
    entry: StoredEntry,
    source: R,
    buf: Vec<u8>,
}

impl<R> Inner<R>
where
    R: ReadAt,
{
    async fn read(&mut self, read_len: usize) -> io::Result<usize> {
        todo!()
    }
}

type PendingFut<R> = Pin<Box<dyn Future<Output = (Inner<R>, io::Result<usize>)> + 'static>>;

enum State<R>
where
    R: ReadAt,
{
    Idle(Inner<R>),
    Pending(PendingFut<R>),
    Transitional,
}

pub struct AsyncEntryReader<R>
where
    R: ReadAt,
{
    state: State<R>,
}

impl<R> AsyncEntryReader<R>
where
    R: ReadAt,
{
    pub fn new(entry: StoredEntry, source: R) -> Self {
        let inner = Inner {
            entry,
            source,
            buf: Default::default(),
        };
        Self {
            state: State::Idle(inner),
        }
    }
}

impl<R> AsyncRead for AsyncEntryReader<R>
where
    R: ReadAt + Unpin + 'static,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        let mut state = State::Transitional;
        std::mem::swap(&mut state, &mut self.state);

        let mut fut = match state {
            State::Idle(mut inner) => {
                let read_len = buf.len();

                Box::pin(async move {
                    let res = inner.read(read_len).await;
                    (inner, res)
                })
            }
            State::Pending(fut) => fut,
            State::Transitional => unreachable!(),
        };
        let res = fut.as_mut().poll(cx);

        match res {
            Poll::Ready((inner, res)) => {
                if let Ok(n) = &res {
                    let n = *n;
                    let dst = &mut buf[..n];
                    let src = &inner.buf[..n];
                    dst.copy_from_slice(src);
                }
                self.state = State::Idle(inner);
                Poll::Ready(res)
            }
            Poll::Pending => {
                self.state = State::Pending(fut);
                Poll::Pending
            }
        }
    }
}
