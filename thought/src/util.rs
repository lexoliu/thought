use std::io::Write;
use std::io::{self, ErrorKind};

use wasmer::MemoryView;

pub struct MemoryViewWrapper<'a> {
    offset: u64,
    view: MemoryView<'a>,
}

impl<'a> MemoryViewWrapper<'a> {
    pub fn new(offset: u64, view: MemoryView<'a>) -> Self {
        Self { offset, view }
    }
}

impl Write for MemoryViewWrapper<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.view
            .write(self.offset, buf)
            .map_err(|error| io::Error::new(ErrorKind::Other, error))?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
