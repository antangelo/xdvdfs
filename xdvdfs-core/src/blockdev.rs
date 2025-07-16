use core::fmt::Display;

use alloc::boxed::Box;
use maybe_async::maybe_async;

use core::error::Error;

/// Represents XGD types and their corresponding XDVDFS partition offsets.
///
/// These values are used to locate the start of the XDVDFS game partition within
/// images.
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
#[repr(u64)]
pub enum XDVDFSOffsets {
    #[default]
    XISO = 0,
    XGD1 = 405798912,
    XGD2 = 265879552,
    XGD3 = 34078720,
}

impl XDVDFSOffsets {
    const ALL: [XDVDFSOffsets; 4] = [
        XDVDFSOffsets::XISO,
        XDVDFSOffsets::XGD1,
        XDVDFSOffsets::XGD2,
        XDVDFSOffsets::XGD3,
    ];
}

impl From<XDVDFSOffsets> for u64 {
    fn from(x: XDVDFSOffsets) -> Self {
        x as u64
    }
}

/// Trait for read operations on some block device containing an XDVDFS filesystem
///
/// Calls to `read` will always be thread safe (that is, no two calls to `read` will
/// be made on the same blockdevice at the same time)
#[cfg(feature = "read")]
#[maybe_async]
pub trait BlockDeviceRead: Send + Sync {
    type ReadError;

    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), Self::ReadError>;
}

/// Trait for write operations on some block device
///
/// Calls to trait methods will always be thread safe (that is, no two calls within the trait will
/// be made on the same blockdevice at the same time)
#[cfg(feature = "write")]
#[maybe_async]
pub trait BlockDeviceWrite: Send + Sync {
    type WriteError;

    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), Self::WriteError>;
    async fn len(&mut self) -> Result<u64, Self::WriteError>;

    async fn is_empty(&mut self) -> Result<bool, Self::WriteError> {
        self.len().await.map(|len| len == 0)
    }
}

/// Block device that eats all write operations, without performing any writes.
/// Used for benchmarking. len() will return the correct value based on any write
/// ops given to the device, but the writes are not persisted and operations return
/// immediately, without yielding.
#[derive(Default, Copy, Clone)]
pub struct NullBlockDevice {
    size: u64,
}

impl NullBlockDevice {
    pub fn write_size_adjustment(&mut self, offset: u64, size: u64) {
        self.size = core::cmp::max(self.size, offset + size);
    }
}

#[cfg(feature = "write")]
#[maybe_async]
impl BlockDeviceWrite for NullBlockDevice {
    type WriteError = core::convert::Infallible;

    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), Self::WriteError> {
        self.write_size_adjustment(offset, buffer.len() as u64);
        Ok(())
    }

    async fn len(&mut self) -> Result<u64, Self::WriteError> {
        Ok(self.size)
    }

    async fn is_empty(&mut self) -> Result<bool, Self::WriteError> {
        Ok(self.size == 0)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct OutOfBounds;

impl Display for OutOfBounds {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("out of bounds")
    }
}

impl Error for OutOfBounds {}

#[maybe_async]
impl BlockDeviceRead for [u8] {
    type ReadError = OutOfBounds;

    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), OutOfBounds> {
        let offset = offset as usize;
        if offset >= self.as_ref().len() {
            return Err(OutOfBounds);
        }

        let size = core::cmp::min(self.as_ref().len() - offset, <[u8]>::len(buffer));
        let range = offset..(offset + size);
        buffer.copy_from_slice(&self[range]);
        Ok(())
    }
}

#[cfg(feature = "write")]
#[maybe_async]
impl BlockDeviceWrite for Box<[u8]> {
    type WriteError = OutOfBounds;

    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), Self::WriteError> {
        let offset: usize = offset.try_into().map_err(|_| OutOfBounds)?;
        let buffer_size = <[u8]>::len(self);
        if offset >= buffer_size || buffer_size - offset < buffer.len() {
            return Err(OutOfBounds);
        }

        self[offset..(offset + buffer.len())].copy_from_slice(buffer);
        Ok(())
    }

    async fn len(&mut self) -> Result<u64, Self::WriteError> {
        Ok(<[u8]>::len(self) as u64)
    }
}

#[cfg(feature = "write")]
#[maybe_async]
impl BlockDeviceWrite for [u8] {
    type WriteError = OutOfBounds;

    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), Self::WriteError> {
        let offset: usize = offset.try_into().map_err(|_| OutOfBounds)?;
        let buffer_size = <[u8]>::len(self);
        if offset >= buffer_size || buffer_size - offset < buffer.len() {
            return Err(OutOfBounds);
        }

        self[offset..(offset + buffer.len())].copy_from_slice(buffer);
        Ok(())
    }

    async fn len(&mut self) -> Result<u64, Self::WriteError> {
        Ok(<[u8]>::len(self) as u64)
    }
}

#[cfg(feature = "read")]
#[maybe_async]
impl<E> BlockDeviceRead for Box<dyn BlockDeviceRead<ReadError = E>> {
    type ReadError = E;

    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), Self::ReadError> {
        self.as_mut().read(offset, buffer).await
    }
}

#[cfg(feature = "write")]
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

pub struct OffsetWrapper<T>
where
    T: BlockDeviceRead + Sized,
{
    pub(crate) inner: T,
    pub(crate) offset: XDVDFSOffsets,
}

impl<T> OffsetWrapper<T>
where
    T: BlockDeviceRead + Sized,
{
    #[maybe_async]
    pub async fn new(
        dev: T,
    ) -> Result<Self, crate::util::Error<<T as BlockDeviceRead>::ReadError>> {
        let mut s = Self {
            inner: dev,
            offset: XDVDFSOffsets::default(),
        };

        for offset in XDVDFSOffsets::ALL {
            s.offset = offset;

            let vol = crate::read::read_volume(&mut s).await;
            if vol.is_ok() {
                return Ok(s);
            }
        }

        Err(crate::util::Error::InvalidVolume)
    }

    pub fn get_offset(&self) -> XDVDFSOffsets {
        self.offset
    }

    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

#[maybe_async]
impl<T> BlockDeviceRead for OffsetWrapper<T>
where
    T: BlockDeviceRead,
{
    type ReadError = T::ReadError;

    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), Self::ReadError> {
        self.inner
            .read(offset + u64::from(self.offset), buffer)
            .await
    }
}

#[cfg(feature = "write")]
#[maybe_async]
impl<T> BlockDeviceWrite for OffsetWrapper<T>
where
    T: BlockDeviceRead + BlockDeviceWrite,
{
    type WriteError = <T as BlockDeviceWrite>::WriteError;

    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), Self::WriteError> {
        self.inner
            .write(offset + u64::from(self.offset), buffer)
            .await
    }

    async fn len(&mut self) -> Result<u64, Self::WriteError> {
        self.inner.len().await
    }
}

#[cfg(feature = "std")]
impl<T> std::io::Seek for OffsetWrapper<T>
where
    T: BlockDeviceRead + std::io::Seek,
{
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        use std::io::SeekFrom;
        match pos {
            SeekFrom::Start(pos) => self
                .inner
                .seek(SeekFrom::Start(u64::from(self.offset) + pos)),
            pos => self.inner.seek(pos),
        }
    }
}

#[cfg(all(feature = "std", feature = "read"))]
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

#[cfg(all(feature = "std", feature = "write"))]
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

#[cfg(all(feature = "std", feature = "write"))]
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
