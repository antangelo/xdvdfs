use core::fmt::{Debug, Display};
use std::path::{Path, PathBuf};

use alloc::{boxed::Box, vec::Vec};

use crate::blockdev::{BlockDeviceRead, BlockDeviceWrite};
use crate::util;

use async_trait::async_trait;

#[derive(Debug)]
pub enum FileType {
    File,
    Directory,
}

#[derive(Debug)]
pub struct FileEntry {
    pub path: PathBuf,
    pub file_type: FileType,
    pub len: u64,
}

pub struct DirectoryTreeEntry {
    pub dir: PathBuf,
    pub listing: Vec<FileEntry>,
}

#[async_trait(?Send)]
pub trait Filesystem<RawHandle: BlockDeviceWrite<E>, E> {
    /// Read a directory, and return a list of entries within it
    async fn read_dir(&mut self, path: &Path) -> Result<Vec<FileEntry>, E>;

    /// Copy the entire contents of file `src` into `dest` at the specified offset
    async fn copy_file_in(
        &mut self,
        src: &Path,
        dest: &mut RawHandle,
        offset: u64,
    ) -> Result<u64, E>;
}

#[cfg(not(target_family = "wasm"))]
pub struct StdFilesystem;

#[cfg(not(target_family = "wasm"))]
#[async_trait(?Send)]
impl Filesystem<std::fs::File, std::io::Error> for StdFilesystem {
    async fn read_dir(&mut self, dir: &Path) -> Result<Vec<FileEntry>, std::io::Error> {
        use std::fs::DirEntry;
        use std::io;

        let listing: io::Result<Vec<DirEntry>> = std::fs::read_dir(dir)?.collect();
        let listing: io::Result<Vec<io::Result<FileEntry>>> = listing?
            .into_iter()
            .map(|de| {
                de.metadata().map(|md| {
                    let file_type = if md.is_dir() {
                        FileType::Directory
                    } else if md.is_file() {
                        FileType::File
                    } else {
                        return Err(io::Error::from(io::ErrorKind::Unsupported));
                    };

                    Ok(FileEntry {
                        path: de.path(),
                        file_type,
                        len: md.len(),
                    })
                })
            })
            .collect();

        let listing: io::Result<Vec<FileEntry>> = listing?.into_iter().collect();
        listing
    }

    async fn copy_file_in(
        &mut self,
        src: &Path,
        dest: &mut std::fs::File,
        offset: u64,
    ) -> Result<u64, std::io::Error> {
        use std::io::{Seek, SeekFrom};

        // FIXME: This is technically a race condition,
        // multiple threads could seek away from this position and corrupt the destination.
        // This needs a mutex to solve, but in practice isn't an issue
        // because create_xdvdfs_image copies files in sequentially.
        let file = std::fs::File::open(src)?;
        dest.seek(SeekFrom::Start(offset))?;
        let mut file = std::io::BufReader::with_capacity(1024 * 1024, file);
        std::io::copy(&mut file, dest)
    }
}

pub struct XDVDFSFilesystem<E, D>
where
    D: BlockDeviceRead<E> + Sized,
{
    dev: D,
    volume: crate::layout::VolumeDescriptor,
    etype: core::marker::PhantomData<E>,
}

impl<E, D> XDVDFSFilesystem<E, D>
where
    D: BlockDeviceRead<E> + Sized,
{
    pub async fn new(mut dev: D) -> Option<XDVDFSFilesystem<E, D>> {
        let volume = crate::read::read_volume(&mut dev).await;

        if let Ok(volume) = volume {
            Some(Self {
                dev,
                volume,
                etype: core::marker::PhantomData,
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

#[async_trait(?Send)]
impl<E, D, W> Filesystem<W, E> for XDVDFSFilesystem<E, D>
where
    E: From<util::Error<E>>,
    D: BlockDeviceRead<E> + Sized,
    W: BlockDeviceWrite<E> + Sized,
{
    async fn read_dir(&mut self, dir: &Path) -> Result<Vec<FileEntry>, E> {
        let path = dir.to_str().ok_or(util::Error::InvalidFileName)?;
        let dirtab = if path == "/" {
            self.volume.root_table
        } else {
            let dirent = self
                .volume
                .root_table
                .walk_path(&mut self.dev, path)
                .await?;
            dirent
                .node
                .dirent
                .dirent_table()
                .ok_or(util::Error::IsNotDirectory)?
        };

        let tree = dirtab.walk_dirent_tree(&mut self.dev).await?;
        let entries: Vec<FileEntry> = tree
            .into_iter()
            .map(|dirent| FileEntry {
                path: dir.join(dirent.get_name()),
                file_type: if dirent.node.dirent.is_directory() {
                    FileType::Directory
                } else {
                    FileType::File
                },
                len: dirent.node.dirent.data.size as u64,
            })
            .collect();

        Ok(entries)
    }

    async fn copy_file_in(&mut self, src: &Path, dest: &mut W, offset: u64) -> Result<u64, E> {
        let path = src.to_str().ok_or(util::Error::InvalidFileName)?;
        let dirent = self
            .volume
            .root_table
            .walk_path(&mut self.dev, path)
            .await?;

        let buf_size = 1024 * 1024;
        let mut buf = alloc::vec![0; buf_size as usize].into_boxed_slice();
        let size = dirent.node.dirent.data.size;

        let mut copied = 0;
        while copied < size {
            let to_copy = core::cmp::min(buf_size, size - copied);
            let slice = &mut buf[0..to_copy.try_into().unwrap()];

            let read_offset = dirent.node.dirent.data.offset(copied)?;
            self.dev.read(read_offset, slice).await?;
            dest.write(offset + (copied as u64), slice).await?;
            copied += to_copy;
        }

        assert_eq!(copied, size);
        Ok(size as u64)
    }
}
