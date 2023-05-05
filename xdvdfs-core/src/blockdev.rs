use alloc::boxed::Box;
use async_trait::async_trait;

/// Trait for read operations on some block device containing an XDVDFS filesystem
/// Calls to `read` will always be thread safe (that is, no two calls to `read` will
/// be made on the same blockdevice at the same time)
#[cfg(feature = "read")]
#[async_trait(?Send)]
pub trait BlockDeviceRead<E> {
    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), E>;
}

/// Trait for write operations on some block device
/// Calls to trait methods will always be thread safe (that is, no two calls within the trait will
/// be made on the same blockdevice at the same time)
#[cfg(feature = "write")]
#[async_trait(?Send)]
pub trait BlockDeviceWrite<E> {
    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), E>;
    async fn len(&mut self) -> Result<u64, E>;

    async fn is_empty(&mut self) -> Result<bool, E> {
        self.len().await.map(|len| len == 0)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct OutOfBounds;

#[async_trait(?Send)]
impl<T: AsRef<[u8]>> BlockDeviceRead<OutOfBounds> for T {
    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), OutOfBounds> {
        let offset = offset as usize;
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
#[async_trait(?Send)]
impl BlockDeviceRead<std::io::Error> for std::fs::File {
    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), std::io::Error> {
        use std::io::Seek;
        self.seek(std::io::SeekFrom::Start(offset))?;
        std::io::Read::read_exact(self, buffer)?;

        Ok(())
    }
}

#[cfg(all(feature = "std", feature = "write"))]
#[async_trait(?Send)]
impl BlockDeviceWrite<std::io::Error> for std::fs::File {
    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), std::io::Error> {
        use std::io::Seek;
        self.seek(std::io::SeekFrom::Start(offset))?;
        std::io::Write::write_all(self, buffer)?;

        Ok(())
    }

    async fn len(&mut self) -> Result<u64, std::io::Error> {
        Ok(self.metadata()?.len())
    }
}
