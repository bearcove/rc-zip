// FIXME: remove
#![allow(unused)]

use oval::Buffer;

use crate::{
    error::Error,
    parse::{DataDescriptorRecord, LocalFileHeaderRecord, Method, StoredEntryInner},
};

use super::FsmResult;

struct EntryReadMetrics {
    uncompressed_size: u64,
    crc32: u32,
}

#[derive(Default)]
enum State {
    ReadLocalHeader {
        buffer: Buffer,
    },
    ReadData {
        hasher: crc32fast::Hasher,
        uncompressed_size: u64,
        header: LocalFileHeaderRecord,
    },
    ReadDataDescriptor {
        buffer: Buffer,
        metrics: EntryReadMetrics,
        header: LocalFileHeaderRecord,
    },
    Validate {
        metrics: EntryReadMetrics,
        header: LocalFileHeaderRecord,
        descriptor: Option<DataDescriptorRecord>,
    },

    #[default]
    Transition,
}

/// A state machine that can parse a zip entry
pub struct EntryFsm {
    state: State,
    entry: StoredEntryInner,
    method: Method,
}

impl EntryFsm {
    fn process(mut self, outbuf: &mut [u8]) -> Result<FsmResult<Self, ()>, Error> {
        todo!()
    }
}
