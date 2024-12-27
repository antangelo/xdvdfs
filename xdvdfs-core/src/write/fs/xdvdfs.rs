use alloc::vec::Vec;
use core::fmt::Debug;
use core::fmt::Display;

#[cfg(not(feature = "sync"))]
use alloc::boxed::Box;

use maybe_async::maybe_async;

use crate::layout::DirectoryEntryNode;
use crate::{
    blockdev::{BlockDeviceRead, BlockDeviceWrite},
    util,
};

use super::{FileEntry, FileType, Filesystem, PathPrefixTree, PathVec};

/// Copy specialization for underlying XDVDFSFilesystem block devices
/// The default implementation of `copy` makes no assumptions about the
/// block devices and performs a buffered copy between them.
/// Override this trait if making assumptions about your block devices
/// allows for optimizing copies between them.
#[maybe_async]
pub trait RWCopier<E, R, W>
where
    R: BlockDeviceRead<E>,
    W: BlockDeviceWrite<E>,
{
    async fn copy(
        offset_in: u64,
        offset_out: u64,
        size: u64,
        src: &mut R,
        dest: &mut W,
    ) -> Result<u64, E> {
        let buf_size = 1024 * 1024;
        let mut buf = alloc::vec![0; buf_size as usize].into_boxed_slice();
        let mut copied = 0;
        while copied < size {
            let to_copy = core::cmp::min(buf_size, size - copied);
            let slice = &mut buf[0..to_copy.try_into().unwrap()];

            src.read(offset_in + copied, slice).await?;
            dest.write(offset_out + copied, slice).await?;
            copied += to_copy;
        }

        assert_eq!(copied, size);
        Ok(copied)
    }
}

/// Default copier specialization, uses the default
/// RWCopier implementation for all inputs
pub struct DefaultCopier<E, R, W> {
    e_type: core::marker::PhantomData<E>,
    r_type: core::marker::PhantomData<R>,
    w_type: core::marker::PhantomData<W>,
}

impl<E, R, W> RWCopier<E, R, W> for DefaultCopier<E, R, W>
where
    R: BlockDeviceRead<E>,
    W: BlockDeviceWrite<E>,
{
}

/// Copier specialization for std::io block devices.
/// This applies to block devices that implement Read, Seek, and Write,
/// or block devices that implement the above and are wrapped by
/// `xdvdfs::blockdev::OffsetWrapper` and specializes the copy to use
/// `std::io::copy`
pub struct StdIOCopier<E, R, W> {
    e_type: core::marker::PhantomData<E>,
    r_type: core::marker::PhantomData<R>,
    w_type: core::marker::PhantomData<W>,
}

#[maybe_async]
impl<E, R, W> RWCopier<E, R, W> for StdIOCopier<E, R, W>
where
    E: From<std::io::Error>,
    R: BlockDeviceRead<E> + std::io::Read + std::io::Seek + Sized,
    W: BlockDeviceWrite<E> + std::io::Write + std::io::Seek,
{
    async fn copy(
        offset_in: u64,
        offset_out: u64,
        size: u64,
        src: &mut R,
        dest: &mut W,
    ) -> Result<u64, E> {
        use std::io::{Read, SeekFrom};
        src.seek(SeekFrom::Start(offset_in))?;
        dest.seek(SeekFrom::Start(offset_out))?;

        std::io::copy(&mut src.by_ref().take(size), dest).map_err(|e| e.into())
    }
}

#[maybe_async]
impl<E, R, W> RWCopier<E, crate::blockdev::OffsetWrapper<R, E>, W>
    for StdIOCopier<E, crate::blockdev::OffsetWrapper<R, E>, W>
where
    E: Send + Sync + From<std::io::Error>,
    R: BlockDeviceRead<E> + std::io::Read + std::io::Seek,
    W: BlockDeviceWrite<E> + std::io::Write + std::io::Seek,
{
    async fn copy(
        offset_in: u64,
        offset_out: u64,
        size: u64,
        src: &mut crate::blockdev::OffsetWrapper<R, E>,
        dest: &mut W,
    ) -> Result<u64, E> {
        use std::io::{Read, Seek, SeekFrom};
        src.seek(SeekFrom::Start(offset_in))?;
        dest.seek(SeekFrom::Start(offset_out))?;

        std::io::copy(&mut src.get_mut().by_ref().take(size), dest).map_err(|e| e.into())
    }
}

/// A Filesystem backed by an XDVDFS block device
/// Reads entries and data from a supplied XDVDFS image
pub struct XDVDFSFilesystem<E, D, W, Copier = DefaultCopier<E, D, W>>
where
    D: BlockDeviceRead<E> + Sized,
    W: BlockDeviceWrite<E>,
{
    dev: D,
    volume: crate::layout::VolumeDescriptor,
    dirent_cache: PathPrefixTree<DirectoryEntryNode>,

    e_type: core::marker::PhantomData<E>,
    w_type: core::marker::PhantomData<W>,
    copier_type: core::marker::PhantomData<Copier>,
}

impl<E, D, W, Copier> XDVDFSFilesystem<E, D, W, Copier>
where
    D: BlockDeviceRead<E> + Sized,
    W: BlockDeviceWrite<E>,
    Copier: RWCopier<E, D, W>,
{
    #[maybe_async]
    pub async fn new(mut dev: D) -> Option<XDVDFSFilesystem<E, D, W, Copier>> {
        let volume = crate::read::read_volume(&mut dev).await;

        if let Ok(volume) = volume {
            Some(Self {
                dev,
                volume,
                dirent_cache: PathPrefixTree::default(),
                e_type: core::marker::PhantomData,
                w_type: core::marker::PhantomData,
                copier_type: core::marker::PhantomData,
            })
        } else {
            None
        }
    }
}

impl<E> From<util::Error<E>> for std::io::Error
where
    E: Send + Sync + Display + Debug + 'static,
{
    fn from(value: util::Error<E>) -> Self {
        Self::new(std::io::ErrorKind::Other, value)
    }
}

#[maybe_async]
impl<E, D, W, Copier> Filesystem<W, E> for XDVDFSFilesystem<E, D, W, Copier>
where
    E: From<util::Error<E>> + Send + Sync,
    D: BlockDeviceRead<E> + Sized,
    W: BlockDeviceWrite<E>,
    Copier: RWCopier<E, D, W> + Send + Sync,
{
    async fn read_dir(&mut self, dir: &PathVec) -> Result<Vec<FileEntry>, E> {
        let (dirtab, cache_node) = if dir.is_root() {
            (self.volume.root_table, &mut self.dirent_cache)
        } else {
            // FIXME: This lookup does not work if `dir` has not been previously
            // found by this function. That is, `dir`'s parent needs to have been queried
            // before `dir` can be queried. This assumption is valid currently as `read_dir`
            // is only used to recursively scan directory contents, but the function contract
            // does not guarantee it generally.
            let (dirent, node) = self
                .dirent_cache
                .lookup_node_mut(dir)
                .and_then(|node| node.record.as_mut())
                .map(|(dirent, subtree)| (*dirent, subtree.as_mut()))
                .ok_or(util::Error::NoDirent)?;
            let dirtab = dirent
                .node
                .dirent
                .dirent_table()
                .ok_or(util::Error::IsNotDirectory)?;
            (dirtab, node)
        };

        let mut tree = dirtab.scan_dirent_tree(&mut self.dev).await?;
        let mut entries = Vec::new();
        while let Some(dirent) = tree.next().await? {
            let name_str = dirent.name_str()?;
            cache_node.insert_tail(&name_str, dirent);
            entries.push(FileEntry {
                name: name_str.into_owned(),
                file_type: if dirent.node.dirent.is_directory() {
                    FileType::Directory
                } else {
                    FileType::File
                },
                len: dirent.node.dirent.data.size as u64,
            });
        }

        Ok(entries)
    }

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut W,
        offset: u64,
        size: u64,
    ) -> Result<u64, E> {
        let dirent = self.dirent_cache.get(src).ok_or(util::Error::NoDirent)?;

        let size_to_copy = core::cmp::min(size, dirent.node.dirent.data.size as u64);
        if size_to_copy == 0 {
            return Ok(0);
        }

        let offset_in = dirent.node.dirent.data.offset(0)?;
        Copier::copy(offset_in, offset, size_to_copy, &mut self.dev, dest).await
    }

    async fn copy_file_buf(
        &mut self,
        src: &PathVec,
        buf: &mut [u8],
        offset: u64,
    ) -> Result<u64, E> {
        let path = src.as_string();
        let dirent = self
            .volume
            .root_table
            .walk_path(&mut self.dev, &path)
            .await?;

        let buf_size: u32 = buf.len().try_into().unwrap();
        let size = dirent.node.dirent.data.size;

        let to_copy = core::cmp::min(buf_size, size);
        let slice = &mut buf[0..to_copy as usize];

        let read_offset = dirent.node.dirent.data.offset(offset)?;
        self.dev.read(read_offset, slice).await?;

        assert!(to_copy <= buf_size);
        buf[(to_copy as usize)..].fill(0);
        Ok(buf_size as u64)
    }
}
