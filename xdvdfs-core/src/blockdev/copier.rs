use alloc::boxed::Box;
use core::convert::Infallible;

use maybe_async::maybe_async;
use thiserror::Error;

use crate::blockdev::{BlockDeviceRead, BlockDeviceWrite, NullBlockDevice};

/// Error type for BlockDeviceCopier operations.
#[derive(Error, Debug, Eq, PartialEq)]
pub enum BlockDeviceCopierError<ReadErr, WriteErr> {
    #[error("failed to read from source block device")]
    BlockDevReadErr(#[source] ReadErr),
    #[error("failed to write to destination block device")]
    BlockDevWriteErr(#[source] WriteErr),
}

/// Copy specialization for underlying XDVDFSFilesystem block devices
/// The default implementation of `copy` makes no assumptions about the
/// block devices and performs a buffered copy between them.
/// Override this trait if making assumptions about your block devices
/// allows for optimizing copies between them.
#[maybe_async]
pub trait RWCopier<R, W>: Default
where
    R: BlockDeviceRead + ?Sized,
    W: BlockDeviceWrite + ?Sized,
{
    async fn copy(
        &mut self,
        offset_in: u64,
        offset_out: u64,
        size: u64,
        src: &mut R,
        dest: &mut W,
    ) -> Result<u64, BlockDeviceCopierError<R::ReadError, W::WriteError>>;

    async fn copy_entire_image(
        &mut self,
        src: &mut R,
        dest: &mut W,
    ) -> Result<(), BlockDeviceCopierError<R::ReadError, W::WriteError>> {
        let image_size = src.image_size().await.map_err(BlockDeviceCopierError::BlockDevReadErr)?;
        self.copy(0, 0, image_size, src, dest).await?;
        Ok(())
    }

    async fn copy_entire_image_by_blocks(
        &mut self,
        src: &mut R,
        dest: &mut W,
        block_size: u64,
        block_copy_finished: &mut (impl FnMut() + Send + Sync),
    ) -> Result<(), BlockDeviceCopierError<R::ReadError, W::WriteError>> {
        let image_size = src.image_size().await.map_err(BlockDeviceCopierError::BlockDevReadErr)?;
        let block_size = core::cmp::min(image_size, block_size);
        let nblocks = image_size.div_ceil(block_size);

        for block in 0..nblocks {
            let offset = block * block_size;
            let remaining = image_size - offset;
            let to_copy = core::cmp::min(remaining, block_size);
            self.copy(
                offset,
                offset,
                to_copy,
                src,
                dest
            ).await?;

            block_copy_finished();
        }

        Ok(())
    }
}

/// Default copier specialization, uses the default
/// RWCopier implementation for all inputs
pub struct DefaultCopier<R: ?Sized, W: ?Sized> {
    buffer: Box<[u8]>,
    r_type: core::marker::PhantomData<R>,
    w_type: core::marker::PhantomData<W>,
}

impl<R: ?Sized, W: ?Sized> DefaultCopier<R, W> {
    pub fn with_buffer_size(buffer_size: usize) -> Self {
        Self {
            buffer: alloc::vec![0u8; buffer_size].into_boxed_slice(),
            r_type: core::marker::PhantomData,
            w_type: core::marker::PhantomData,
        }
    }
}

impl<R: ?Sized, W: ?Sized> Default for DefaultCopier<R, W> {
    fn default() -> Self {
        Self::with_buffer_size(1024 * 1024)
    }
}

#[maybe_async]
impl<R, W> RWCopier<R, W> for DefaultCopier<R, W>
where
    R: BlockDeviceRead + ?Sized,
    W: BlockDeviceWrite + ?Sized,
{
    async fn copy(
        &mut self,
        offset_in: u64,
        offset_out: u64,
        size: u64,
        src: &mut R,
        dest: &mut W,
    ) -> Result<u64, BlockDeviceCopierError<R::ReadError, W::WriteError>> {
        let buf_size = self.buffer.len() as u64;
        let mut copied = 0;
        while copied < size {
            let to_copy = core::cmp::min(buf_size, size - copied);
            let slice = &mut self.buffer[0..(to_copy as usize)];

            src.read(offset_in + copied, slice)
                .await
                .map_err(BlockDeviceCopierError::BlockDevReadErr)?;
            dest.write(offset_out + copied, slice)
                .await
                .map_err(BlockDeviceCopierError::BlockDevWriteErr)?;
            copied += to_copy;
        }

        assert_eq!(copied, size);
        Ok(copied)
    }
}

/// Copier specialization for std::io block devices.
/// This applies to block devices that implement Read, Seek, and Write,
/// or block devices that implement the above and are wrapped by
/// `xdvdfs::blockdev::OffsetWrapper` and specializes the copy to use
/// `std::io::copy`
pub struct StdIOCopier<R: ?Sized, W: ?Sized> {
    r_type: core::marker::PhantomData<R>,
    w_type: core::marker::PhantomData<W>,
}

impl<R: ?Sized, W: ?Sized> Default for StdIOCopier<R, W> {
    fn default() -> Self {
        Self {
            r_type: core::marker::PhantomData,
            w_type: core::marker::PhantomData,
        }
    }
}

#[maybe_async]
impl<R, W> RWCopier<R, W> for StdIOCopier<R, W>
where
    R: BlockDeviceRead<ReadError = std::io::Error> + std::io::Read + std::io::Seek + Sized,
    W: BlockDeviceWrite<WriteError = std::io::Error> + std::io::Write + std::io::Seek + ?Sized,
{
    async fn copy(
        &mut self,
        offset_in: u64,
        offset_out: u64,
        size: u64,
        src: &mut R,
        dest: &mut W,
    ) -> Result<u64, BlockDeviceCopierError<R::ReadError, W::WriteError>> {
        use std::io::{Read, SeekFrom};
        src.seek(SeekFrom::Start(offset_in))
            .map_err(BlockDeviceCopierError::BlockDevReadErr)?;
        dest.seek(SeekFrom::Start(offset_out))
            .map_err(BlockDeviceCopierError::BlockDevWriteErr)?;

        // Arbitrarily assign copy errors to the write side,
        // we can't differentiate them anyway
        std::io::copy(&mut src.by_ref().take(size), dest)
            .map_err(BlockDeviceCopierError::BlockDevWriteErr)
    }

    async fn copy_entire_image(
        &mut self,
        src: &mut R,
        dest: &mut W,
    ) -> Result<(), BlockDeviceCopierError<R::ReadError, W::WriteError>> {
        use std::io::SeekFrom;
        src.seek(SeekFrom::Start(0))
            .map_err(BlockDeviceCopierError::BlockDevReadErr)?;
        dest.seek(SeekFrom::Start(0))
            .map_err(BlockDeviceCopierError::BlockDevWriteErr)?;

        // Arbitrarily assign copy errors to the write side,
        // we can't differentiate them anyway
        std::io::copy(src, dest)
            .map_err(BlockDeviceCopierError::BlockDevWriteErr)?;
        Ok(())
    }
}

#[maybe_async]
impl<R, W> RWCopier<crate::blockdev::OffsetWrapper<R>, W>
    for StdIOCopier<crate::blockdev::OffsetWrapper<R>, W>
where
    R: BlockDeviceRead<ReadError = std::io::Error> + std::io::Read + std::io::Seek + Sized,
    W: BlockDeviceWrite<WriteError = std::io::Error> + std::io::Write + std::io::Seek + ?Sized,
{
    async fn copy(
        &mut self,
        offset_in: u64,
        offset_out: u64,
        size: u64,
        src: &mut crate::blockdev::OffsetWrapper<R>,
        dest: &mut W,
    ) -> Result<u64, BlockDeviceCopierError<std::io::Error, std::io::Error>> {
        use std::io::{Read, Seek, SeekFrom};
        src.seek(SeekFrom::Start(offset_in))
            .map_err(BlockDeviceCopierError::BlockDevReadErr)?;
        dest.seek(SeekFrom::Start(offset_out))
            .map_err(BlockDeviceCopierError::BlockDevWriteErr)?;

        // Arbitrarily assign copy errors to the write side,
        // we can't differentiate them anyway
        std::io::copy(&mut src.get_mut().by_ref().take(size), dest)
            .map_err(BlockDeviceCopierError::BlockDevWriteErr)
    }

    async fn copy_entire_image(
        &mut self,
        src: &mut crate::blockdev::OffsetWrapper<R>,
        dest: &mut W,
    ) -> Result<(), BlockDeviceCopierError<R::ReadError, W::WriteError>> {
        use std::io::{Seek, SeekFrom};
        src.seek(SeekFrom::Start(0))
            .map_err(BlockDeviceCopierError::BlockDevReadErr)?;
        dest.seek(SeekFrom::Start(0))
            .map_err(BlockDeviceCopierError::BlockDevWriteErr)?;

        // Arbitrarily assign copy errors to the write side,
        // we can't differentiate them anyway
        std::io::copy(src.get_mut(), dest)
            .map_err(BlockDeviceCopierError::BlockDevWriteErr)?;
        Ok(())
    }
}

/// Null copier specialization
/// Works only on NullBlockDevice, copying is a no-op
pub struct NullCopier<R: ?Sized> {
    r_type: core::marker::PhantomData<R>,
}

impl<R: ?Sized> Default for NullCopier<R> {
    fn default() -> Self {
        Self {
            r_type: core::marker::PhantomData,
        }
    }
}

#[maybe_async]
impl<R> RWCopier<R, NullBlockDevice> for NullCopier<R>
where
    R: BlockDeviceRead + ?Sized,
{
    async fn copy(
        &mut self,
        _offset_in: u64,
        offset_out: u64,
        size: u64,
        _src: &mut R,
        dest: &mut NullBlockDevice,
    ) -> Result<u64, BlockDeviceCopierError<R::ReadError, Infallible>> {
        dest.write_size_adjustment(offset_out, size);
        Ok(size)
    }
}

#[cfg(test)]
mod test {
    use futures::executor::block_on;

    use crate::blockdev::NullBlockDevice;

    use super::{DefaultCopier, NullCopier, RWCopier};

    #[test]
    fn test_write_xdvdfs_default_copier() {
        let mut dest = alloc::vec![0u8; 20];
        let mut src = alloc::vec![0xffu8; 20];
        src[0..3].fill(0x2e);

        let mut copier = DefaultCopier::with_buffer_size(5);
        let res = block_on(copier.copy(1, 2, 20 - 2, src.as_mut_slice(), dest.as_mut_slice()));
        assert_eq!(res, Ok(20 - 2));
        assert_eq!(dest[0..2], [0, 0]);
        assert_eq!(src[1..19], dest[2..]);
    }

    #[test]
    fn test_write_xdvdfs_null_copier() {
        let mut src = alloc::vec![0xffu8; 20];
        let mut dest = NullBlockDevice::default();

        let mut copier = NullCopier::default();
        let res = block_on(copier.copy(1, 2, 20 - 2, src.as_mut_slice(), &mut dest));
        assert_eq!(res, Ok(20 - 2));
        assert_eq!(dest.len_blocking(), 20);
    }
}

#[cfg(all(test, feature = "std"))]
mod test_std {
    use std::io::Cursor;

    use futures::executor::block_on;

    use crate::blockdev::{OffsetWrapper, XDVDFSOffsets};

    use super::{RWCopier, StdIOCopier};

    #[test]
    fn test_write_xdvdfs_std_copier() {
        let dest = std::vec![0u8; 20];
        let mut dest = Cursor::new(dest);

        let mut src = alloc::vec![0xffu8; 20];
        src[0..3].fill(0x2e);
        let mut src = Cursor::new(src);

        let mut copier = StdIOCopier::default();
        let res = block_on(copier.copy(1, 2, 20 - 2, &mut src, &mut dest));
        assert!(res.is_ok_and(|sz| sz == 20 - 2));
        assert_eq!(dest.get_ref()[0..2], [0, 0]);
        assert_eq!(src.get_ref()[1..19], dest.get_ref()[2..]);
    }

    #[test]
    fn test_write_xdvdfs_std_copier_offset_wrapper() {
        let dest = std::vec![0u8; 20];
        let mut dest = Cursor::new(dest);

        let mut src = alloc::vec![0xffu8; 20 + XDVDFSOffsets::XGD3 as usize];
        src[0..(3 + XDVDFSOffsets::XGD3 as usize)].fill(0x2e);
        let src = Cursor::new(src);
        let mut src = OffsetWrapper::new_with_provided_offset(src, XDVDFSOffsets::XGD3);

        let mut copier = StdIOCopier::default();
        let res = block_on(copier.copy(1, 2, 20 - 2, &mut src, &mut dest));
        assert!(res.is_ok_and(|sz| sz == 20 - 2));
        assert_eq!(dest.get_ref()[0..2], [0, 0]);

        let start = XDVDFSOffsets::XGD3 as usize + 1;
        assert_eq!(
            src.get_ref().get_ref()[start..(start + 18)],
            dest.get_ref()[2..]
        );
    }
}
