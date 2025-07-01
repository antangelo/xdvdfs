use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use core::fmt::Debug;

use crate::blockdev::BlockDeviceWrite;

use maybe_async::maybe_async;

mod memory;
mod remap;
mod sector_linear;
mod xdvdfs;

pub mod path;
pub use path::*;

pub use memory::*;
pub use remap::*;
pub use sector_linear::*;
pub use xdvdfs::*;

#[cfg(not(target_family = "wasm"))]
mod stdfs;

#[cfg(not(target_family = "wasm"))]
pub use stdfs::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileType {
    File,
    Directory,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileEntry {
    pub name: String,
    pub file_type: FileType,
    pub len: u64,
}

#[derive(Clone, Debug)]
pub struct DirectoryTreeEntry {
    pub dir: PathVec,
    pub listing: Vec<FileEntry>,
}

/// A trait for filesystem hierarchies, representing any filesystem
/// structure with hierarchical directories.
///
/// This trait allows for scanning a given directory within a filesystem
/// for a list of its entries and entry metadata.
#[maybe_async]
pub trait FilesystemHierarchy: Send + Sync {
    type Error;

    /// Read a directory, and return a list of entries within it
    async fn read_dir(&mut self, path: &PathVec) -> Result<Vec<FileEntry>, Self::Error>;

    /// Display a filesystem path as a String
    fn path_to_string(&self, path: &PathVec) -> String {
        path.as_string()
    }
}

#[maybe_async]
impl<E> FilesystemHierarchy for Box<dyn FilesystemHierarchy<Error = E>> {
    type Error = E;

    async fn read_dir(&mut self, path: &PathVec) -> Result<Vec<FileEntry>, E> {
        self.as_mut().read_dir(path).await
    }

    fn path_to_string(&self, path: &PathVec) -> String {
        self.as_ref().path_to_string(path)
    }
}

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
