use alloc::boxed::Box;
use maybe_async::maybe_async;

/// Trait for write operations on some block device
///
/// Calls to trait methods will always be thread safe (that is, no two calls within the trait will
/// be made on the same blockdevice at the same time)
#[maybe_async]
pub trait BlockDeviceWrite: Send + Sync {
    type WriteError;

    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), Self::WriteError>;
    async fn len(&mut self) -> Result<u64, Self::WriteError>;

    async fn is_empty(&mut self) -> Result<bool, Self::WriteError> {
        self.len().await.map(|len| len == 0)
    }
}

#[maybe_async]
impl<E> BlockDeviceWrite for Box<dyn BlockDeviceWrite<WriteError = E>> {
    type WriteError = E;

    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), E> {
        self.as_mut().write(offset, buffer).await
    }

    async fn len(&mut self) -> Result<u64, E> {
        self.as_mut().len().await
    }
}

#[cfg(feature = "std")]
#[maybe_async]
impl BlockDeviceWrite for std::fs::File {
    type WriteError = std::io::Error;

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

#[cfg(feature = "std")]
#[maybe_async]
impl BlockDeviceWrite for std::io::BufWriter<std::fs::File> {
    type WriteError = std::io::Error;

    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), std::io::Error> {
        use std::io::Seek;
        self.seek(std::io::SeekFrom::Start(offset))?;
        std::io::Write::write_all(self, buffer)?;

        Ok(())
    }

    async fn len(&mut self) -> Result<u64, std::io::Error> {
        Ok(self.get_mut().metadata()?.len())
    }
}
