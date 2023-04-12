#[cfg(feature = "read")]
pub trait BlockDeviceRead<E> {
    fn read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), E>;
}

#[cfg(feature = "write")]
pub trait BlockDeviceWrite {
    fn write(&mut self, offset: usize, buffer: &[u8]);
}

#[derive(Copy, Clone, Debug)]
pub struct OutOfBounds;

impl<T: AsRef<[u8]>> BlockDeviceRead<OutOfBounds> for T {
    fn read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), OutOfBounds> {
        if offset >= self.as_ref().len() {
            return Err(OutOfBounds);
        }

        let size = core::cmp::min(self.as_ref().len() - offset, buffer.len());
        let range = offset..(offset + size);
        buffer.copy_from_slice(&self.as_ref()[range]);
        Ok(())
    }
}

#[cfg(all(feature = "std", feature = "read"))]
impl BlockDeviceRead<std::io::Error> for std::fs::File {
    fn read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), std::io::Error> {
        use std::io::Seek;
        self.seek(std::io::SeekFrom::Start(offset as u64))?;
        std::io::Read::read_exact(self, buffer)?;

        Ok(())
    }
}

#[cfg(all(feature = "std", feature = "write"))]
impl BlockDeviceWrite for std::fs::File {
    fn write(&mut self, offset: usize, buffer: &[u8]) {
        use std::io::Seek;
        self.seek(std::io::SeekFrom::Start(offset as u64)).unwrap();
        std::io::Write::write_all(self, buffer).unwrap();
    }
}
