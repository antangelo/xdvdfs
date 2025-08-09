use alloc::boxed::Box;
use maybe_async::maybe_async;

/// Trait for read operations on some block device containing an XDVDFS filesystem
///
/// Calls to `read` will always be thread safe (that is, no two calls to `read` will
/// be made on the same blockdevice at the same time)
#[maybe_async]
pub trait BlockDeviceRead: Send + Sync {
    type ReadError;

    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), Self::ReadError>;
}

#[maybe_async]
impl<E> BlockDeviceRead for Box<dyn BlockDeviceRead<ReadError = E>> {
    type ReadError = E;

    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), Self::ReadError> {
        self.as_mut().read(offset, buffer).await
    }
}

#[cfg(feature = "std")]
#[maybe_async]
impl<R> BlockDeviceRead for R
where
    R: std::io::Read + std::io::Seek + Send + Sync,
{
    type ReadError = std::io::Error;

    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), std::io::Error> {
        self.seek(std::io::SeekFrom::Start(offset))?;
        std::io::Read::read_exact(self, buffer)?;

        Ok(())
    }
}
