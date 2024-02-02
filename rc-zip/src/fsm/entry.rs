// FIXME: remove
#![allow(unused)]

use oval::Buffer;

use crate::{DataDescriptorRecord, LocalFileHeaderRecord};

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
        metrics: EntryReadMetrics,
        header: LocalFileHeaderRecord,
        buffer: Buffer,
    },
    Validate {
        metrics: EntryReadMetrics,
        header: LocalFileHeaderRecord,
        descriptor: Option<DataDescriptorRecord>,
    },

    /// Done!
    Done,

    #[default]
    Transition,
}

/// A state machine that can parse a zip entry
pub struct EntryFsm {
    state: State,
}
