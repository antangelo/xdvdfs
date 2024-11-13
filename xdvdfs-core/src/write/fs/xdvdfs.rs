use alloc::vec::Vec;
use core::fmt::Debug;
use core::fmt::Display;

#[cfg(not(feature = "sync"))]
use alloc::boxed::Box;

use maybe_async::maybe_async;

use crate::{
    blockdev::{BlockDeviceRead, BlockDeviceWrite},
    util,
};

use super::{FileEntry, FileType, Filesystem, PathVec};

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
    #[maybe_async]
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

#[maybe_async]
impl<E, D, W> Filesystem<W, E> for XDVDFSFilesystem<E, D>
where
    E: From<util::Error<E>> + Send + Sync,
    D: BlockDeviceRead<E> + Sized,
    W: BlockDeviceWrite<E> + Sized,
{
    async fn read_dir(&mut self, dir: &PathVec) -> Result<Vec<FileEntry>, E> {
        let dirtab = if dir.is_root() {
            self.volume.root_table
        } else {
            let path = dir.as_string();
            let dirent = self
                .volume
                .root_table
                .walk_path(&mut self.dev, &path)
                .await?;
            dirent
                .node
                .dirent
                .dirent_table()
                .ok_or(util::Error::IsNotDirectory)?
        };

        let tree = dirtab.walk_dirent_tree(&mut self.dev).await?;
        let entries: Result<Vec<FileEntry>, util::Error<E>> = tree
            .into_iter()
            .map(|dirent| {
                Ok(FileEntry {
                    name: dirent.name_str()?.into_owned(),
                    file_type: if dirent.node.dirent.is_directory() {
                        FileType::Directory
                    } else {
                        FileType::File
                    },
                    len: dirent.node.dirent.data.size as u64,
                })
            })
            .collect();

        Ok(entries?)
    }

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut W,
        offset: u64,
        size: u64,
    ) -> Result<u64, E> {
        let path = src.as_string();
        let dirent = self
            .volume
            .root_table
            .walk_path(&mut self.dev, &path)
            .await?;

        let buf_size = 1024 * 1024;
        let mut buf = alloc::vec![0; buf_size as usize].into_boxed_slice();
        let size = size as u32;
        assert_eq!(dirent.node.dirent.data.size(), size);

        // TODO: Find a way to specialize this for Files, where more efficient std::io::copy
        // routines can be used (specifically on Linux)
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

        let buf_size = buf.len() as u32;
        let size = dirent.node.dirent.data.size;

        let to_copy = core::cmp::min(buf_size, size);
        let slice = &mut buf[0..to_copy.try_into().unwrap()];

        let read_offset = dirent.node.dirent.data.offset(offset as u32)?;
        self.dev.read(read_offset, slice).await?;

        assert!(to_copy <= buf_size);
        buf[(to_copy as usize)..].fill(0);
        Ok(buf_size as u64)
    }
}
