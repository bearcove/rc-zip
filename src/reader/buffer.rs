use log::*;
use std::io::Read;

/// A wrapper around [circular::Buffer] that keeps track of how many bytes we've read since
/// initialization or the last reset.
pub(crate) struct Buffer {
    pub(crate) buffer: circular::Buffer,
    pub(crate) read_bytes: u64,
}

impl Buffer {
    /// creates a new buffer with the specified capacity
    pub(crate) fn with_capacity(size: usize) -> Self {
        Self {
            buffer: circular::Buffer::with_capacity(size),
            read_bytes: 0,
        }
    }

    /// resets the buffer (so that data() returns an empty slice,
    /// and space() returns the full capacity), along with th e
    /// read bytes counter.
    pub(crate) fn reset(&mut self) {
        self.read_bytes = 0;
        self.buffer.reset();
    }

    /// returns the number of read bytes since the last reset
    pub(crate) fn read_bytes(&self) -> u64 {
        self.read_bytes
    }

    /// returns a slice with all the available data
    pub(crate) fn data(&self) -> &[u8] {
        self.buffer.data()
    }

    /// returns how much data can be read from the buffer
    pub(crate) fn available_data(&self) -> usize {
        self.buffer.available_data()
    }

    /// advances the position tracker
    ///
    /// if the position gets past the buffer's half,
    /// this will call `shift()` to move the remaining data
    /// to the beginning of the buffer
    pub(crate) fn consume(&mut self, count: usize) -> usize {
        self.buffer.consume(count)
    }

    /// fill that buffer from the given Read
    pub(crate) fn read(&mut self, rd: &mut dyn Read) -> Result<usize, std::io::Error> {
        if self.buffer.available_space() == 0 {
            debug!("uh oh, buffer has no available space!")
        }

        match rd.read(self.buffer.space()) {
            Ok(written) => {
                self.read_bytes += written as u64;
                self.buffer.fill(written);
                Ok(written)
            }
            Err(e) => Err(e),
        }
    }

    pub(crate) fn read_offset(&self, offset: u64) -> u64 {
        self.read_bytes + offset
    }
}
