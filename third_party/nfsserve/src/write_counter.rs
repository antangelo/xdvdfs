#![allow(dead_code)]
use std::io::Write;

// from https://stackoverflow.com/questions/42187591/how-to-keep-track-of-how-many-bytes-written-when-using-stdiowrite

pub struct WriteCounter<W> {
    inner: W,
    count: usize,
}

impl<W> WriteCounter<W>
where
    W: Write,
{
    pub fn new(inner: W) -> Self {
        WriteCounter { inner, count: 0 }
    }

    pub fn into_inner(self) -> W {
        self.inner
    }

    pub fn bytes_written(&self) -> usize {
        self.count
    }
}

impl<W> Write for WriteCounter<W>
where
    W: Write,
{
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let res = self.inner.write(buf);
        if let Ok(size) = res {
            self.count += size
        }
        res
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
