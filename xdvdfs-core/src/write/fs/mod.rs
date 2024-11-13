use core::fmt::Debug;
use core::slice::Iter;
use std::borrow::ToOwned;
use std::format;
use std::path::{Path, PathBuf};

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::blockdev::BlockDeviceWrite;

use maybe_async::maybe_async;

mod remap;
mod sector_linear;
mod xdvdfs;

pub use remap::*;
pub use sector_linear::*;
pub use xdvdfs::*;

#[cfg(not(target_family = "wasm"))]
mod stdfs;

#[cfg(not(target_family = "wasm"))]
pub use stdfs::*;

#[derive(Copy, Clone, Debug)]
pub enum FileType {
    File,
    Directory,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PathVec {
    components: Vec<String>,
}

type PathVecIter<'a> = Iter<'a, String>;

#[derive(Clone, Debug)]
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

impl PathVec {
    pub fn as_path_buf(&self, prefix: &Path) -> PathBuf {
        let suffix = PathBuf::from_iter(self.components.iter());
        prefix.join(suffix)
    }

    pub fn is_root(&self) -> bool {
        self.components.is_empty()
    }

    pub fn iter(&self) -> PathVecIter {
        self.components.iter()
    }

    pub fn from_base(prefix: &Self, suffix: &str) -> Self {
        let mut path = prefix.clone();
        path.components.push(suffix.to_owned());
        path
    }

    pub fn as_string(&self) -> String {
        format!("/{}", self.components.join("/"))
    }

    pub fn suffix(&self, prefix: &Self) -> Self {
        let mut components = Vec::new();
        let mut i1 = self.iter();
        let mut i2 = prefix.iter();

        loop {
            let c1 = i1.next();
            let c2 = i2.next();

            if let Some(component) = c1 {
                if let Some(component2) = c2 {
                    assert_eq!(component, component2);
                } else {
                    components.push(component.clone());
                }
            } else {
                return Self { components };
            }
        }
    }
}

impl<'a> FromIterator<&'a str> for PathVec {
    fn from_iter<T: IntoIterator<Item = &'a str>>(iter: T) -> Self {
        let components = iter
            .into_iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_owned())
            .collect();
        Self { components }
    }
}

#[maybe_async]
pub trait Filesystem<RawHandle: BlockDeviceWrite<RHError>, E, RHError: Into<E> = E>:
    Send + Sync
{
    /// Read a directory, and return a list of entries within it
    ///
    /// Other functions in this trait are guaranteed to be called with PathVecs
    /// returned from this function call, possibly with the FileEntry name appended.
    async fn read_dir(&mut self, path: &PathVec) -> Result<Vec<FileEntry>, E>;

    /// Copy the entire contents of file `src` into `dest` at the specified offset
    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut RawHandle,
        offset: u64,
        size: u64,
    ) -> Result<u64, E>;

    /// Copy the contents of file `src` into `buf` at the specified offset
    /// Not required for normal usage
    async fn copy_file_buf(
        &mut self,
        _src: &PathVec,
        _buf: &mut [u8],
        _offset: u64,
    ) -> Result<u64, E>;

    /// Display a filesystem path as a String
    fn path_to_string(&self, path: &PathVec) -> String {
        path.as_string()
    }
}

#[maybe_async]
impl<E: Send + Sync, R: BlockDeviceWrite<E>> Filesystem<R, E> for Box<dyn Filesystem<R, E>> {
    async fn read_dir(&mut self, path: &PathVec) -> Result<Vec<FileEntry>, E> {
        self.as_mut().read_dir(path).await
    }

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut R,
        offset: u64,
        size: u64,
    ) -> Result<u64, E> {
        self.as_mut().copy_file_in(src, dest, offset, size).await
    }

    async fn copy_file_buf(
        &mut self,
        src: &PathVec,
        buf: &mut [u8],
        offset: u64,
    ) -> Result<u64, E> {
        self.as_mut().copy_file_buf(src, buf, offset).await
    }
}
