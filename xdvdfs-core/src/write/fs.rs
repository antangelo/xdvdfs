use core::fmt::{Debug, Display};
use core::slice::Iter;
use std::borrow::ToOwned;
use std::format;
use std::path::{Path, PathBuf};

use alloc::string::String;
use alloc::vec::Vec;

#[cfg(not(feature = "sync"))]
use alloc::boxed::Box;

use crate::blockdev::{BlockDeviceRead, BlockDeviceWrite};
use crate::{layout, util};

use maybe_async::maybe_async;

#[derive(Debug)]
pub enum FileType {
    File,
    Directory,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PathVec {
    components: Vec<String>,
}

type PathVecIter<'a> = Iter<'a, String>;

#[derive(Debug)]
pub struct FileEntry {
    pub name: String,
    pub file_type: FileType,
    pub len: u64,
}

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
}

#[maybe_async]
pub trait Filesystem<RawHandle: BlockDeviceWrite<E>, E>: Send + Sync {
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

#[cfg(not(target_family = "wasm"))]
pub struct StdFilesystem {
    root: PathBuf,
}

#[cfg(not(target_family = "wasm"))]
impl StdFilesystem {
    pub fn create(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
        }
    }
}

#[cfg(not(target_family = "wasm"))]
#[maybe_async]
impl<T> Filesystem<T, std::io::Error> for StdFilesystem
where
    T: std::io::Write + std::io::Seek + BlockDeviceWrite<std::io::Error>,
{
    async fn read_dir(&mut self, dir: &PathVec) -> Result<Vec<FileEntry>, std::io::Error> {
        use alloc::string::ToString;
        use std::fs::DirEntry;
        use std::io;

        let dir = dir.as_path_buf(&self.root);
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

                    let name = de
                        .path()
                        .file_name()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_string())
                        .ok_or(io::Error::from(io::ErrorKind::Unsupported))?;

                    Ok(FileEntry {
                        name,
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
        src: &PathVec,
        dest: &mut T,
        offset: u64,
        _size: u64,
    ) -> Result<u64, std::io::Error> {
        use std::io::SeekFrom;

        // FIXME: This is technically a race condition,
        // multiple threads could seek away from this position and corrupt the destination.
        // This needs a mutex to solve, but in practice isn't an issue
        // because create_xdvdfs_image copies files in sequentially.
        dest.seek(SeekFrom::Start(offset))?;

        let src = src.as_path_buf(&self.root);
        let file = std::fs::File::open(src)?;
        let mut file = std::io::BufReader::with_capacity(1024 * 1024, file);
        std::io::copy(&mut file, dest)
    }

    async fn copy_file_buf(
        &mut self,
        src: &PathVec,
        buf: &mut [u8],
        offset: u64,
    ) -> Result<u64, std::io::Error> {
        use std::io::{Read, Seek, SeekFrom};

        let src = src.as_path_buf(&self.root);
        let mut file = std::fs::File::open(src)?;
        file.seek(SeekFrom::Start(offset))?;

        let bytes_read = Read::read(&mut file, buf)?;
        buf[bytes_read..].fill(0);
        Ok(buf.len() as u64)
    }

    fn path_to_string(&self, path: &PathVec) -> String {
        let path = path.as_path_buf(&self.root);
        format!("{:?}", path)
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

#[derive(Clone, Debug)]
pub enum SectorLinearBlockContents {
    RawData(Box<[u8; layout::SECTOR_SIZE as usize]>),
    File(PathVec, u64),
    Empty,
}

#[derive(Clone, Debug)]
pub struct SectorLinearBlockDevice<E> {
    contents: alloc::collections::BTreeMap<u64, SectorLinearBlockContents>,

    err_t: core::marker::PhantomData<E>,
}

pub struct SectorLinearBlockFilesystem<'a, E, W: BlockDeviceWrite<E>, F: Filesystem<W, E>> {
    fs: &'a mut F,

    err_t: core::marker::PhantomData<E>,
    bdev_t: core::marker::PhantomData<W>,
}

impl<'a, E, W: BlockDeviceWrite<E>, F: Filesystem<W, E>> SectorLinearBlockFilesystem<'a, E, W, F> {
    pub fn new(fs: &'a mut F) -> Self {
        Self {
            fs,

            err_t: core::marker::PhantomData,
            bdev_t: core::marker::PhantomData,
        }
    }
}

#[maybe_async]
impl<E: Send + Sync> BlockDeviceWrite<E> for SectorLinearBlockDevice<E> {
    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), E> {
        let mut remaining = buffer.len();
        let mut buffer_pos = 0;

        let mut sector = offset / layout::SECTOR_SIZE as u64;

        let offset = offset % layout::SECTOR_SIZE as u64;
        assert_eq!(offset, 0);

        while remaining > 0 {
            let to_write = core::cmp::min(layout::SECTOR_SIZE as usize, remaining);

            let sector_buf = alloc::vec![0; layout::SECTOR_SIZE as usize];
            let sector_buf: Box<[u8]> = sector_buf.into_boxed_slice();
            let mut sector_buf: Box<[u8; layout::SECTOR_SIZE as usize]> = unsafe {
                Box::from_raw(Box::into_raw(sector_buf) as *mut [u8; layout::SECTOR_SIZE as usize])
            };

            sector_buf[0..to_write].copy_from_slice(&buffer[buffer_pos..(buffer_pos + to_write)]);

            if self
                .contents
                .insert(sector, SectorLinearBlockContents::RawData(sector_buf))
                .is_some()
            {
                unimplemented!("Overwriting sectors is not implemented");
            }

            remaining -= to_write;
            buffer_pos += to_write;
            sector += 1;
        }

        Ok(())
    }

    async fn len(&mut self) -> Result<u64, E> {
        Ok(self
            .contents
            .last_key_value()
            .map(|(sector, contents)| {
                *sector * layout::SECTOR_SIZE as u64
                    + match contents {
                        SectorLinearBlockContents::RawData(_) => layout::SECTOR_SIZE,
                        SectorLinearBlockContents::File(_, _) => layout::SECTOR_SIZE,
                        SectorLinearBlockContents::Empty => 0,
                    } as u64
            })
            .unwrap_or(0))
    }
}

#[maybe_async]
impl<'a, E, W, F> Filesystem<SectorLinearBlockDevice<E>, E>
    for SectorLinearBlockFilesystem<'a, E, W, F>
where
    W: BlockDeviceWrite<E>,
    F: Filesystem<W, E>,
    E: Send + Sync,
{
    async fn read_dir(&mut self, path: &PathVec) -> Result<Vec<FileEntry>, E> {
        self.fs.read_dir(path).await
    }

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut SectorLinearBlockDevice<E>,
        offset: u64,
        size: u64,
    ) -> Result<u64, E> {
        let sector = offset / layout::SECTOR_SIZE as u64;
        let offset = offset % layout::SECTOR_SIZE as u64;
        assert_eq!(offset, 0);

        let mut sector_span = size / layout::SECTOR_SIZE as u64;
        if size % layout::SECTOR_SIZE as u64 > 0 {
            sector_span += 1;
        }

        for i in 0..sector_span {
            if dest
                .contents
                .insert(sector + i, SectorLinearBlockContents::File(src.clone(), i))
                .is_some()
            {
                unimplemented!("Overwriting sectors is not implemented");
            }
        }

        Ok(size)
    }

    async fn copy_file_buf(
        &mut self,
        _src: &PathVec,
        _buf: &mut [u8],
        _offset: u64,
    ) -> Result<u64, E> {
        unimplemented!();
    }
}

impl<E> SectorLinearBlockDevice<E> {
    pub fn num_sectors(&self) -> usize {
        self.contents.len()
    }
}

impl<E> Default for SectorLinearBlockDevice<E> {
    fn default() -> Self {
        Self {
            contents: alloc::collections::BTreeMap::new(),
            err_t: core::marker::PhantomData,
        }
    }
}

impl<E> core::ops::Index<u64> for SectorLinearBlockDevice<E> {
    type Output = SectorLinearBlockContents;

    fn index(&self, index: u64) -> &Self::Output {
        self.contents
            .get(&index)
            .unwrap_or(&SectorLinearBlockContents::Empty)
    }
}

#[cfg(feature = "ciso_support")]
pub struct CisoSectorInput<'a, E: Send + Sync, W: BlockDeviceWrite<E>, F: Filesystem<W, E>> {
    linear: SectorLinearBlockDevice<E>,
    fs: SectorLinearBlockFilesystem<'a, E, W, F>,

    bdev_t: core::marker::PhantomData<W>,
}

#[cfg(feature = "ciso_support")]
impl<'a, E, W, F> CisoSectorInput<'a, E, W, F>
where
    W: BlockDeviceWrite<E>,
    F: Filesystem<W, E>,
    E: Send + Sync,
{
    pub fn new(
        bdev: SectorLinearBlockDevice<E>,
        fs: SectorLinearBlockFilesystem<'a, E, W, F>,
    ) -> Self {
        Self {
            linear: bdev,
            fs,
            bdev_t: core::marker::PhantomData,
        }
    }
}

#[cfg(feature = "ciso_support")]
#[maybe_async]
impl<'a, E, W, F> ciso::write::SectorReader<E> for CisoSectorInput<'a, E, W, F>
where
    W: BlockDeviceWrite<E>,
    F: Filesystem<W, E>,
    E: Send + Sync,
{
    async fn size(&mut self) -> Result<u64, E> {
        self.linear.len().await
    }

    async fn read_sector(&mut self, sector: usize, sector_size: u32) -> Result<Vec<u8>, E> {
        let mut buf = alloc::vec![0; sector_size as usize];

        match &self.linear[sector as u64] {
            SectorLinearBlockContents::Empty => {}
            SectorLinearBlockContents::RawData(data) => {
                buf.copy_from_slice(data.as_slice());
            }
            SectorLinearBlockContents::File(path, sector_idx) => {
                let bytes_read = self
                    .fs
                    .fs
                    .copy_file_buf(path, &mut buf, sector_size as u64 * sector_idx)
                    .await?;
                assert_eq!(bytes_read, sector_size as u64);
            }
        };

        Ok(buf)
    }
}
