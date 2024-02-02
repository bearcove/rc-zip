use rc_zip::{
    fsm::{EntryFsm, FsmResult},
    parse::StoredEntry,
};
use std::io;

pub(crate) struct EntryReader<R>
where
    R: io::Read,
{
    rd: R,
    fsm: Option<EntryFsm>,
}

impl<R> EntryReader<R>
where
    R: io::Read,
{
    pub(crate) fn new(entry: &StoredEntry, rd: R) -> Self {
        Self {
            rd,
            fsm: Some(EntryFsm::new(entry.method(), entry.inner)),
        }
    }
}

impl<R> io::Read for EntryReader<R>
where
    R: io::Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut fsm = match self.fsm.take() {
            Some(fsm) => fsm,
            None => return Ok(0),
        };

        if fsm.wants_read() {
            tracing::trace!("fsm wants read");
            let n = self.rd.read(fsm.space())?;
            tracing::trace!("giving fsm {} bytes", n);
            fsm.fill(n);
        } else {
            tracing::trace!("fsm does not want read");
        }

        match fsm.process(buf)? {
            FsmResult::Continue((fsm, outcome)) => {
                self.fsm = Some(fsm);
                if outcome.bytes_written > 0 {
                    Ok(outcome.bytes_written)
                } else {
                    // loop, it happens
                    self.read(buf)
                }
            }
            FsmResult::Done(()) => {
                // neat!
                Ok(0)
            }
        }
    }
}
