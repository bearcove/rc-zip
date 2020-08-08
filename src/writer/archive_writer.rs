use std::io::{BufWriter, Write};

pub struct ArchiveWriter<W>
where
    W: Write,
{
    #[allow(unused)]
    writer: BufWriter<W>,
}

impl<W> ArchiveWriter<W>
where
    W: Write,
{
    pub fn new(writer: W) -> Self {
        Self {
            writer: BufWriter::new(writer),
        }
    }
}
