use std::{error::Error, fmt::Display, future::Future, path::Path, time::SystemTime};

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
    #[must_use]
    fn add(&mut self, inode: u64, is_dir: bool, name: &str) -> bool;
}

/// Implements a filesystem that can be served by some protocol (NFS or FUSE).
pub trait Filesystem: Send + Sync {
    fn lookup(
        &self,
        parent: u64,
        filename: &str,
    ) -> impl Future<Output = Result<FileAttribute, FilesystemError>> + Send;
    fn getattr(
        &self,
        inode: u64,
    ) -> impl Future<Output = Result<FileAttribute, FilesystemError>> + Send;

    /// Read `size` bytes at `offset` from file, return data and whether or not EOF was reached.
    fn read(
        &self,
        inode: u64,
        offset: u64,
        size: u64,
    ) -> impl Future<Output = Result<(Vec<u8>, bool), FilesystemError>> + Send;

    /// Read directory entries into `filler` from `offset`, return `true` if finished.
    fn readdir(
        &self,
        inode: u64,
        offset: u64,
        filler: &mut dyn ReadDirFiller,
    ) -> impl Future<Output = Result<bool, FilesystemError>> + Send;

    fn is_writeable(
        &self,
        inode: u64,
    ) -> impl Future<Output = Result<bool, FilesystemError>> + Send {
        async move { self.getattr(inode).await.map(|attr| attr.is_writeable) }
    }
}

impl<F: Filesystem> Filesystem for &F {
    fn lookup(
        &self,
        parent: u64,
        filename: &str,
    ) -> impl Future<Output = Result<FileAttribute, FilesystemError>> + Send {
        (*self).lookup(parent, filename)
    }

    fn getattr(
        &self,
        inode: u64,
    ) -> impl Future<Output = Result<FileAttribute, FilesystemError>> + Send {
        (*self).getattr(inode)
    }

    fn read(
        &self,
        inode: u64,
        offset: u64,
        size: u64,
    ) -> impl Future<Output = Result<(Vec<u8>, bool), FilesystemError>> + Send {
        (*self).read(inode, offset, size)
    }

    fn readdir(
        &self,
        inode: u64,
        offset: u64,
        filler: &mut dyn ReadDirFiller,
    ) -> impl Future<Output = Result<bool, FilesystemError>> + Send {
        (*self).readdir(inode, offset, filler)
    }

    fn is_writeable(
        &self,
        inode: u64,
    ) -> impl Future<Output = Result<bool, FilesystemError>> + Send {
        (*self).is_writeable(inode)
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
