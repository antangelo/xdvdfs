#[cfg(feature = "read")]
pub trait BlockDeviceRead {
    fn read(&mut self, offset: usize, buffer: &mut [u8]);
}

#[cfg(feature = "write")]
pub trait BlockDeviceWrite {
    fn write(&mut self, offset: usize, buffer: &[u8]);
}

#[cfg(all(feature = "std", feature = "read"))]
impl BlockDeviceRead for std::fs::File {
    fn read(&mut self, offset: usize, buffer: &mut [u8]) {
        use std::io::Seek;
        self.seek(std::io::SeekFrom::Start(offset as u64)).unwrap();
        std::io::Read::read(self, buffer).unwrap();
    }
}

#[cfg(all(feature = "std", feature = "write"))]
impl BlockDeviceWrite for std::fs::File {
    fn write(&mut self, offset: usize, buffer: &[u8]) {
        use std::io::Seek;
        self.seek(std::io::SeekFrom::Start(offset as u64)).unwrap();
        std::io::Write::write(self, buffer).unwrap();
    }
}
