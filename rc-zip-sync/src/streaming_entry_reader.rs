use oval::Buffer;
use rc_zip::{
    fsm::{EntryFsm, FsmResult},
    parse::Entry,
};
use std::io::{self, Write};
use tracing::trace;

pub struct StreamingEntryReader<R> {
    entry: Entry,
    rd: R,
    state: State,
}

#[derive(Default)]
#[allow(clippy::large_enum_variant)]
enum State {
    Reading {
        remain: Buffer,
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
    R: io::Read,
{
    pub(crate) fn new(remain: Buffer, entry: Entry, rd: R) -> Self {
        Self {
            rd,
            entry,
            state: State::Reading {
                remain,
                fsm: EntryFsm::new(None),
            },
        }
    }
}

impl<R> io::Read for StreamingEntryReader<R>
where
    R: io::Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match std::mem::take(&mut self.state) {
            State::Reading {
                mut remain,
                mut fsm,
            } => {
                if fsm.wants_read() {
                    trace!("fsm wants read");
                    if remain.available_data() > 0 {
                        trace!(
                            "remain has {} data bytes available",
                            remain.available_data(),
                        );
                        let n = remain.read(fsm.space())?;
                        trace!("giving fsm {} bytes from remain", n);
                        fsm.fill(n);
                    } else {
                        let n = self.rd.read(fsm.space())?;
                        trace!("giving fsm {} bytes from rd", n);
                        fsm.fill(n);
                    }
                } else {
                    trace!("fsm does not want read");
                }

                match fsm.process(buf)? {
                    FsmResult::Continue((fsm, outcome)) => {
                        self.state = State::Reading { remain, fsm };

                        if outcome.bytes_written > 0 {
                            Ok(outcome.bytes_written)
                        } else if outcome.bytes_read == 0 {
                            // that's EOF, baby!
                            Ok(0)
                        } else {
                            // loop, it happens
                            self.read(buf)
                        }
                    }
                    FsmResult::Done(mut fsm_remain) => {
                        // if our remain still has remaining data, it goes after
                        // what the fsm just gave back
                        if remain.available_data() > 0 {
                            fsm_remain.grow(fsm_remain.capacity() + remain.available_data());
                            fsm_remain.write_all(remain.data())?;
                            drop(remain)
                        }

                        // FIXME: read the next local file header here
                        self.state = State::Finished { remain: fsm_remain };

                        // neat!
                        Ok(0)
                    }
                }
            }
            State::Finished { remain } => {
                // wait for them to call finished
                self.state = State::Finished { remain };
                Ok(0)
            }
            State::Transition => unreachable!(),
        }
    }
}

impl<R> StreamingEntryReader<R> {
    /// Return entry information for this reader
    #[inline(always)]
    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    /// Finish reading this entry, returning the next streaming entry reader, if
    /// any. This panics if the entry is not fully read.
    pub fn finish(self) -> Option<StreamingEntryReader<R>> {
        match self.state {
            State::Reading { .. } => {
                panic!("finish called before entry is fully read")
            }
            State::Finished { .. } => {
                todo!("read local file header for next entry")
            }
            State::Transition => unreachable!(),
        }
    }
}
