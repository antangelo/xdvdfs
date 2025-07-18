use alloc::vec::Vec;
use core::convert::Infallible;
use core::error::Error;
use core::fmt::Debug;
use core::fmt::Display;

#[cfg(not(feature = "sync"))]
use alloc::boxed::Box;

use maybe_async::maybe_async;

use crate::blockdev::NullBlockDevice;
use crate::layout::DirectoryEntryNode;
use crate::{
    blockdev::{BlockDeviceRead, BlockDeviceWrite},
    util,
};

use super::FilesystemCopier;
use super::FilesystemHierarchy;
use super::{FileEntry, FileType, PathPrefixTree, PathVec};

/// Error type for XDVDFSFilesystem operations
/// A BlockDev error is an error that occurred during a copy operation
/// in the respective block device side.
/// A FilesystemReadErr is an error that occurred while traversing the
/// underlying XDVDFS filesystem.
#[derive(Debug)]
pub enum XDVDFSFilesystemError<ReadErr, WriteErr> {
    FilesystemReadErr(util::Error<ReadErr>),
    BlockDevReadErr(ReadErr),
    BlockDevWriteErr(WriteErr),
}

impl<ReadErr, WriteErr> XDVDFSFilesystemError<ReadErr, WriteErr> {
    fn to_str(&self) -> &str {
        match self {
            Self::FilesystemReadErr(_) => "Failed to read XDVDFS filesystem",
            Self::BlockDevReadErr(_) => "Failed to read from block device",
            Self::BlockDevWriteErr(_) => "Failed to write to block device",
        }
    }
}

impl<ReadErr: Display, WriteErr: Display> Display for XDVDFSFilesystemError<ReadErr, WriteErr> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.to_str())?;
        f.write_str(": ")?;
        match self {
            Self::FilesystemReadErr(ref e) => Display::fmt(e, f),
            Self::BlockDevReadErr(ref e) => Display::fmt(e, f),
            Self::BlockDevWriteErr(ref e) => Display::fmt(e, f),
        }
    }
}

impl<ReadErr, WriteErr> Error for XDVDFSFilesystemError<ReadErr, WriteErr>
where
    ReadErr: Debug + Display + Error + 'static,
    WriteErr: Debug + Display + Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FilesystemReadErr(ref e) => Some(e),
            Self::BlockDevReadErr(ref e) => Some(e),
            Self::BlockDevWriteErr(ref e) => Some(e),
        }
    }
}

/// Copy specialization for underlying XDVDFSFilesystem block devices
/// The default implementation of `copy` makes no assumptions about the
/// block devices and performs a buffered copy between them.
/// Override this trait if making assumptions about your block devices
/// allows for optimizing copies between them.
#[maybe_async]
pub trait RWCopier<R, W>
where
    R: BlockDeviceRead + ?Sized,
    W: BlockDeviceWrite + ?Sized,
{
    async fn copy(
        offset_in: u64,
        offset_out: u64,
        size: u64,
        src: &mut R,
        dest: &mut W,
    ) -> Result<u64, XDVDFSFilesystemError<R::ReadError, W::WriteError>> {
        let buf_size = 1024 * 1024;
        let mut buf = alloc::vec![0; buf_size as usize].into_boxed_slice();
        let mut copied = 0;
        while copied < size {
            let to_copy = core::cmp::min(buf_size, size - copied);
            let slice = &mut buf[0..(to_copy as usize)];

            src.read(offset_in + copied, slice)
                .await
                .map_err(XDVDFSFilesystemError::BlockDevReadErr)?;
            dest.write(offset_out + copied, slice)
                .await
                .map_err(XDVDFSFilesystemError::BlockDevWriteErr)?;
            copied += to_copy;
        }

        assert_eq!(copied, size);
        Ok(copied)
    }
}

/// Default copier specialization, uses the default
/// RWCopier implementation for all inputs
pub struct DefaultCopier<R: ?Sized, W: ?Sized> {
    r_type: core::marker::PhantomData<R>,
    w_type: core::marker::PhantomData<W>,
}

impl<R, W> RWCopier<R, W> for DefaultCopier<R, W>
where
    R: BlockDeviceRead + ?Sized,
    W: BlockDeviceWrite + ?Sized,
{
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

#[maybe_async]
impl<R, W> RWCopier<R, W> for StdIOCopier<R, W>
where
    R: BlockDeviceRead<ReadError = std::io::Error> + std::io::Read + std::io::Seek + Sized,
    W: BlockDeviceWrite<WriteError = std::io::Error> + std::io::Write + std::io::Seek + ?Sized,
{
    async fn copy(
        offset_in: u64,
        offset_out: u64,
        size: u64,
        src: &mut R,
        dest: &mut W,
    ) -> Result<u64, XDVDFSFilesystemError<std::io::Error, std::io::Error>> {
        use std::io::{Read, SeekFrom};
        src.seek(SeekFrom::Start(offset_in))
            .map_err(XDVDFSFilesystemError::BlockDevReadErr)?;
        dest.seek(SeekFrom::Start(offset_out))
            .map_err(XDVDFSFilesystemError::BlockDevWriteErr)?;

        // Arbitrarily assign copy errors to the write side,
        // we can't differentiate them anyway
        std::io::copy(&mut src.by_ref().take(size), dest)
            .map_err(XDVDFSFilesystemError::BlockDevWriteErr)
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
        offset_in: u64,
        offset_out: u64,
        size: u64,
        src: &mut crate::blockdev::OffsetWrapper<R>,
        dest: &mut W,
    ) -> Result<u64, XDVDFSFilesystemError<std::io::Error, std::io::Error>> {
        use std::io::{Read, Seek, SeekFrom};
        src.seek(SeekFrom::Start(offset_in))
            .map_err(XDVDFSFilesystemError::BlockDevReadErr)?;
        dest.seek(SeekFrom::Start(offset_out))
            .map_err(XDVDFSFilesystemError::BlockDevWriteErr)?;

        // Arbitrarily assign copy errors to the write side,
        // we can't differentiate them anyway
        std::io::copy(&mut src.get_mut().by_ref().take(size), dest)
            .map_err(XDVDFSFilesystemError::BlockDevWriteErr)
    }
}

/// A Filesystem backed by an XDVDFS block device
/// Reads entries and data from a supplied XDVDFS image
pub struct XDVDFSFilesystem<D, W, Copier = DefaultCopier<D, W>>
where
    D: BlockDeviceRead + Sized,
    W: BlockDeviceWrite + ?Sized,
{
    dev: D,
    volume: crate::layout::VolumeDescriptor,
    dirent_cache: PathPrefixTree<DirectoryEntryNode>,

    w_type: core::marker::PhantomData<W>,
    copier_type: core::marker::PhantomData<Copier>,
}

impl<D, W, Copier> XDVDFSFilesystem<D, W, Copier>
where
    D: BlockDeviceRead + Sized,
    W: BlockDeviceWrite + ?Sized,
    Copier: RWCopier<D, W>,
{
    #[maybe_async]
    pub async fn new(mut dev: D) -> Option<XDVDFSFilesystem<D, W, Copier>> {
        let volume = crate::read::read_volume(&mut dev).await;

        if let Ok(volume) = volume {
            Some(Self {
                dev,
                volume,
                dirent_cache: PathPrefixTree::default(),
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
        Self::other(value)
    }
}

#[maybe_async]
impl<D, W, Copier> FilesystemHierarchy for XDVDFSFilesystem<D, W, Copier>
where
    D: BlockDeviceRead + Sized,
    W: BlockDeviceWrite + ?Sized,
    Copier: RWCopier<D, W> + Send + Sync,
{
    type Error = util::Error<D::ReadError>;

    async fn read_dir(&mut self, dir: &PathVec) -> Result<Vec<FileEntry>, Self::Error> {
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
        while let Some(dirent) = tree.next_entry().await? {
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

    async fn clear_cache(&mut self) -> Result<(), Self::Error> {
        self.dirent_cache = PathPrefixTree::default();

        Ok(())
    }
}

#[maybe_async]
impl<D, W, Copier> FilesystemCopier<W> for XDVDFSFilesystem<D, W, Copier>
where
    D: BlockDeviceRead + Sized,
    W: BlockDeviceWrite + ?Sized,
    Copier: RWCopier<D, W> + Send + Sync,
{
    type Error = XDVDFSFilesystemError<D::ReadError, W::WriteError>;

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut W,
        input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, Self::Error> {
        let dirent = self
            .dirent_cache
            .get(src)
            .ok_or(XDVDFSFilesystemError::FilesystemReadErr(
                util::Error::NoDirent,
            ))?;

        let size_to_copy = core::cmp::min(size, dirent.node.dirent.data.size as u64);
        if size_to_copy == 0 {
            return Ok(0);
        }

        let input_offset = dirent
            .node
            .dirent
            .data
            .offset(input_offset)
            .map_err(XDVDFSFilesystemError::FilesystemReadErr)?;
        Copier::copy(
            input_offset,
            output_offset,
            size_to_copy,
            &mut self.dev,
            dest,
        )
        .await
    }
}

/// Null copier specialization
/// Works only on NullBlockDevice, copying is a no-op
pub struct NullCopier<R: ?Sized> {
    r_type: core::marker::PhantomData<R>,
}

#[maybe_async]
impl<R> RWCopier<R, NullBlockDevice> for NullCopier<R>
where
    R: BlockDeviceRead + ?Sized,
{
    async fn copy(
        _offset_in: u64,
        offset_out: u64,
        size: u64,
        _src: &mut R,
        dest: &mut NullBlockDevice,
    ) -> Result<u64, XDVDFSFilesystemError<R::ReadError, Infallible>> {
        dest.write_size_adjustment(offset_out, size);
        Ok(size)
    }
}
