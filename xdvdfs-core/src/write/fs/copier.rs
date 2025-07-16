use alloc::boxed::Box;
use maybe_async::maybe_async;

use super::PathVec;
use crate::blockdev::BlockDeviceWrite;

/// A trait for copying data out of a filesystem.
///
/// Allows for copying data from a specified filesystem path
/// into an output block device, specialized by the block device type.
/// Multiple implementations of this trait allow the filesystem to be
/// used to create images on various output types.
#[maybe_async]
pub trait FilesystemCopier<BDW: BlockDeviceWrite + ?Sized>: Send + Sync {
    type Error;

    /// Copy the entire contents of file `src` into `dest` at the specified offset
    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut BDW,
        input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, Self::Error>;
}

#[maybe_async]
impl<E, BDW: BlockDeviceWrite> FilesystemCopier<BDW> for Box<dyn FilesystemCopier<BDW, Error = E>> {
    type Error = E;

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut BDW,
        input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, E> {
        self.as_mut()
            .copy_file_in(src, dest, input_offset, output_offset, size)
            .await
    }
}

#[maybe_async]
impl<E, BDW, F> FilesystemCopier<BDW> for &mut F
where
    BDW: BlockDeviceWrite + ?Sized,
    F: FilesystemCopier<BDW, Error = E> + ?Sized,
{
    type Error = E;

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut BDW,
        input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, E> {
        (**self)
            .copy_file_in(src, dest, input_offset, output_offset, size)
            .await
    }
}
