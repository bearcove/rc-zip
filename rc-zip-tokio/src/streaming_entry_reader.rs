use oval::Buffer;
use pin_project_lite::pin_project;
use rc_zip::{
    error::{Error, FormatError},
    fsm::{EntryFsm, FsmResult},
    parse::Entry,
};
use std::{io, pin::Pin, task};
use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};
use tracing::trace;

pin_project! {
    /// Reads a zip entry based on a local header. Some information is missing,
    /// not all name encodings may work, and only by reading it in its entirety
    /// can you move on to the next entry.
    ///
    /// However, it only requires an [AsyncRead], and does not need to seek.
    pub struct StreamingEntryReader<R> {
        entry: Entry,
        #[pin]
        rd: R,
        state: State,
    }
}

#[derive(Default)]
#[allow(clippy::large_enum_variant)]
enum State {
    Reading {
        fsm: EntryFsm,
    },
    Finished {
        /// remaining buffer for next entry
        remain: Buffer,
    },
    #[default]
    Transition,
}

impl<R> StreamingEntryReader<R>
where
    R: AsyncRead,
{
    pub(crate) fn new(fsm: EntryFsm, entry: Entry, rd: R) -> Self {
        Self {
            entry,
            rd,
            state: State::Reading { fsm },
        }
    }
}

impl<R> AsyncRead for StreamingEntryReader<R>
where
    R: AsyncRead,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> task::Poll<io::Result<()>> {
        let this = self.as_mut().project();

        trace!("reading from streaming entry reader");

        match std::mem::take(this.state) {
            State::Reading { mut fsm } => {
                if fsm.wants_read() {
                    trace!("fsm wants read");
                    let mut buf = ReadBuf::new(fsm.space());
                    match this.rd.poll_read(cx, &mut buf) {
                        task::Poll::Ready(res) => res?,
                        task::Poll::Pending => {
                            *this.state = State::Reading { fsm };
                            return task::Poll::Pending;
                        }
                    }
                    let n = buf.filled().len();

                    trace!("giving fsm {} bytes from rd", n);
                    fsm.fill(n);
                } else {
                    trace!("fsm does not want read");
                }

                match fsm.process(buf.initialize_unfilled())? {
                    FsmResult::Continue((fsm, outcome)) => {
                        trace!("fsm wants to continue");
                        *this.state = State::Reading { fsm };

                        if outcome.bytes_written > 0 {
                            trace!("bytes have been written");
                            buf.advance(outcome.bytes_written);
                        } else if outcome.bytes_read == 0 {
                            trace!("no bytes have been written or read");
                            // that's EOF, baby!
                        } else {
                            trace!("read some bytes, hopefully will write more later");
                            // loop, it happens
                            return self.poll_read(cx, buf);
                        }
                    }
                    FsmResult::Done(remain) => {
                        *this.state = State::Finished { remain };

                        // neat!
                    }
                }
            }
            State::Finished { remain } => {
                // wait for them to call finish
                *this.state = State::Finished { remain };
            }
            State::Transition => unreachable!(),
        }
        Ok(()).into()
    }
}

impl<R> StreamingEntryReader<R>
where
    R: AsyncRead + Unpin,
{
    /// Return entry information for this reader
    #[inline(always)]
    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    /// Finish reading this entry, returning the next streaming entry reader, if
    /// any. This panics if the entry is not fully read.
    ///
    /// If this returns None, there's no entries left.
    pub async fn finish(mut self) -> Result<Option<StreamingEntryReader<R>>, Error> {
        trace!("finishing streaming entry reader");

        if matches!(self.state, State::Reading { .. }) {
            // this should transition to finished if there's no data
            _ = self.read(&mut [0u8; 1]).await?;
        }

        match self.state {
            State::Reading { .. } => {
                panic!("entry not fully read");
            }
            State::Finished { remain } => {
                // parse the next entry, if any
                let mut fsm = EntryFsm::new(None, Some(remain));

                loop {
                    if fsm.wants_read() {
                        let n = self.rd.read(fsm.space()).await?;
                        trace!("read {} bytes into buf for first zip entry", n);
                        fsm.fill(n);
                    }

                    match fsm.process_till_header() {
                        Ok(Some(entry)) => {
                            let entry = entry.clone();
                            return Ok(Some(StreamingEntryReader::new(fsm, entry, self.rd)));
                        }
                        Ok(None) => {
                            // needs more turns
                        }
                        Err(e) => match e {
                            Error::Format(FormatError::InvalidLocalHeader) => {
                                // we probably reached the end of central directory!
                                // TODO: we should probably check for the end of central directory
                                return Ok(None);
                            }
                            _ => return Err(e),
                        },
                    }
                }
            }
            State::Transition => unreachable!(),
        }
    }
}
