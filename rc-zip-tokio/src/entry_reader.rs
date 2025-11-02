use std::{io, pin::Pin, task};

use pin_project_lite::pin_project;
use rc_zip::{
    fsm::{EntryFsm, FsmResult},
    parse::Entry,
};
use tokio::io::{AsyncRead, ReadBuf};

pin_project! {
    pub(crate) struct EntryReader<R>
    where
        R: AsyncRead,
    {
        #[pin]
        rd: R,
        fsm: Option<EntryFsm>,
    }
}

impl<R> EntryReader<R>
where
    R: AsyncRead,
{
    pub(crate) fn new<F>(entry: &Entry, get_reader: F) -> Self
    where
        F: Fn(u64) -> R,
    {
        Self {
            rd: get_reader(entry.header_offset),
            fsm: Some(EntryFsm::new(Some(entry.clone()), None)),
        }
    }
}

impl<R> AsyncRead for EntryReader<R>
where
    R: AsyncRead,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> task::Poll<std::io::Result<()>> {
        let mut this = self.as_mut().project();

        loop {
            let mut fsm = match this.fsm.take() {
                Some(fsm) => fsm,
                None => return Ok(()).into(),
            };

            let filled_bytes;
            if fsm.wants_read() {
                tracing::trace!(space_avail = fsm.space().len(), "fsm wants read");
                let mut buf = ReadBuf::new(fsm.space());
                match this.rd.as_mut().poll_read(cx, &mut buf) {
                    task::Poll::Ready(res) => res?,
                    task::Poll::Pending => {
                        *this.fsm = Some(fsm);
                        return task::Poll::Pending;
                    }
                }
                let n = buf.filled().len();

                tracing::trace!("read {} bytes", n);
                fsm.fill(n);
                filled_bytes = n;
            } else {
                tracing::trace!("fsm does not want read");
                filled_bytes = 0;
            }

            match fsm.process(buf.initialize_unfilled())? {
                FsmResult::Continue((fsm, outcome)) => {
                    *this.fsm = Some(fsm);
                    if outcome.bytes_written > 0 {
                        tracing::trace!("wrote {} bytes", outcome.bytes_written);
                        buf.advance(outcome.bytes_written);
                    } else if filled_bytes > 0 || outcome.bytes_read > 0 {
                        // progress was made, keep reading
                        continue;
                    } else {
                        return Err(io::Error::other("entry reader: no progress")).into();
                    }
                }
                FsmResult::Done(_) => {
                    // neat!
                }
            }
            return Ok(()).into();
        }
    }
}
