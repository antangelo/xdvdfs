use std::{error::Error, fmt::Display, path::Path, time::SystemTime};

use async_trait::async_trait;
use tokio::runtime::Runtime;

#[cfg(all(unix, feature = "fuse"))]
pub mod fuse;

pub mod nfs;

#[derive(Copy, Clone, Default, Debug)]
#[non_exhaustive]
pub enum FilesystemErrorKind {
    #[default]
    NotImplemented,
    IOError,
    NotDirectory,
    IsDirectory,
    NoEntry,
}

#[derive(Default, Debug)]
pub struct FilesystemError {
    kind: FilesystemErrorKind,
    source: Option<Box<dyn Error + 'static>>,
}

impl FilesystemErrorKind {
    pub fn with<E: Error + 'static>(self, source: E) -> FilesystemError {
        FilesystemError {
            kind: self,
            source: Some(Box::from(source)),
        }
    }
}

impl From<FilesystemErrorKind> for FilesystemError {
    fn from(value: FilesystemErrorKind) -> Self {
        Self {
            kind: value,
            source: None,
        }
    }
}

impl Display for FilesystemErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotImplemented => f.write_str("Not Implemented"),
            Self::IOError => f.write_str("IO Error"),
            Self::NotDirectory => f.write_str("Not a Directory"),
            Self::IsDirectory => f.write_str("Is a Directory"),
            Self::NoEntry => f.write_str("No Entry"),
        }
    }
}

impl Display for FilesystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.kind.fmt(f)?;

        if let Some(ref err) = self.source {
            f.write_str(": ")?;
            err.fmt(f)?;
        }

        Ok(())
    }
}

impl Error for FilesystemError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|x| x.as_ref())
    }
}

pub struct FileAttribute {
    pub inode: u64,
    pub byte_size: u64,
    pub block_size: u64,
    pub is_dir: bool,
    pub is_writeable: bool,
    pub atime: SystemTime,
    pub mtime: SystemTime,
    pub ctime: SystemTime,
    pub crtime: SystemTime,
}

pub trait ReadDirFiller: Send + Sync {
    // Add entry to the dir filler. If this returns true, exit `readdir` early.
    #[must_use]
    fn add(&mut self, inode: u64, is_dir: bool, name: &str) -> bool;
}

/// Implements a filesystem that can be served by some protocol (NFS or FUSE).
#[async_trait]
pub trait Filesystem: Send + Sync {
    async fn lookup(&self, parent: u64, filename: &str) -> Result<FileAttribute, FilesystemError>;

    async fn getattr(&self, inode: u64) -> Result<FileAttribute, FilesystemError>;

    /// Read `size` bytes at `offset` from file, return data and whether or not EOF was reached.
    async fn read(
        &self,
        inode: u64,
        offset: u64,
        size: u64,
    ) -> Result<(Vec<u8>, bool), FilesystemError>;

    /// Read directory entries into `filler` from `offset`, return `true` if finished.
    async fn readdir(
        &self,
        inode: u64,
        offset: u64,
        filler: &mut dyn ReadDirFiller,
    ) -> Result<bool, FilesystemError>;

    async fn is_writeable(&self, inode: u64) -> Result<bool, FilesystemError> {
        self.getattr(inode).await.map(|attr| attr.is_writeable)
    }
}

#[async_trait]
impl<F: Filesystem> Filesystem for &F {
    async fn lookup(&self, parent: u64, filename: &str) -> Result<FileAttribute, FilesystemError> {
        (**self).lookup(parent, filename).await
    }

    async fn getattr(&self, inode: u64) -> Result<FileAttribute, FilesystemError> {
        (**self).getattr(inode).await
    }

    async fn read(
        &self,
        inode: u64,
        offset: u64,
        size: u64,
    ) -> Result<(Vec<u8>, bool), FilesystemError> {
        (**self).read(inode, offset, size).await
    }

    async fn readdir(
        &self,
        inode: u64,
        offset: u64,
        filler: &mut dyn ReadDirFiller,
    ) -> Result<bool, FilesystemError> {
        (**self).readdir(inode, offset, filler).await
    }

    async fn is_writeable(&self, inode: u64) -> Result<bool, FilesystemError> {
        (**self).is_writeable(inode).await
    }
}

#[derive(Copy, Clone)]
pub struct TopLevelOptions {
    pub fork: bool,
}

pub trait FSMounter: Default {
    fn process_args(
        &mut self,
        mount_point: Option<&Path>,
        src: &Path,
        options: &[String],
    ) -> anyhow::Result<TopLevelOptions>;

    fn mount<F: Filesystem + 'static>(
        self,
        fs: F,
        rt: &Runtime,
        mount_point: Option<&Path>,
    ) -> anyhow::Result<()>;
}
