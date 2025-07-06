use std::{
    fs::Metadata,
    path::{Path, PathBuf},
};

use async_trait::async_trait;

use crate::{
    fsproto::{FileAttribute, Filesystem, FilesystemError, FilesystemErrorKind, ReadDirFiller},
    hostutils::metadata_to_attr,
    inode::INodeCache,
};

use super::{OverlayProvider, OverlayProviderInstance, ProviderInstance};

const ROOT_INODE: u64 = 1;
const FILE_INODE: u64 = 2;

/// File provider for overlayfs root nodes
/// Provides the host file for any file in the source directory (1-1 mapping)
pub struct OverlayFSFileProvider;

#[async_trait]
impl OverlayProvider for OverlayFSFileProvider {
    async fn matches_entry(&self, entry: &Path) -> Option<String> {
        let metadata = entry.metadata().ok()?;
        if !metadata.is_dir() && !metadata.is_file() {
            return None;
        }

        let file_name = entry.file_name()?.to_str()?;
        Some(file_name.to_string())
    }

    async fn instantiate(
        &self,
        entry: &Path,
        name: &str,
    ) -> Result<ProviderInstance, FilesystemError> {
        let file = entry.to_path_buf();
        let meta = file
            .metadata()
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;

        let is_dir = meta.is_dir();
        let inode = if is_dir { 1 } else { 2 };

        let filesystem: Box<dyn OverlayProviderInstance> = if is_dir {
            Box::new(HostDirectoryProviderInstance {
                root: file,
                meta,
                inode_cache: std::sync::RwLock::new(INodeCache::default()),
            })
        } else {
            Box::new(HostFileProviderInstance {
                filename: name.to_string(),
                file,
                meta,
            })
        };

        Ok(ProviderInstance {
            filesystem,
            inode,
            is_dir,
        })
    }
}

pub struct HostFileProviderInstance {
    filename: String,
    file: PathBuf,
    meta: Metadata,
}

pub struct HostDirectoryProviderInstance {
    root: PathBuf,
    meta: Metadata,
    inode_cache: std::sync::RwLock<INodeCache<PathBuf>>,
}

#[async_trait]
impl OverlayProviderInstance for HostFileProviderInstance {}

#[async_trait]
impl OverlayProviderInstance for HostDirectoryProviderInstance {}

#[async_trait]
impl Filesystem for HostFileProviderInstance {
    async fn lookup(&self, parent: u64, filename: &str) -> Result<FileAttribute, FilesystemError> {
        if parent == ROOT_INODE && filename == self.filename {
            Ok(metadata_to_attr(FILE_INODE, &self.meta))
        } else {
            Err(FilesystemError::from(FilesystemErrorKind::NoEntry))
        }
    }

    async fn getattr(&self, inode: u64) -> Result<FileAttribute, FilesystemError> {
        match inode {
            FILE_INODE => Ok(metadata_to_attr(FILE_INODE, &self.meta)),
            _ => Err(FilesystemError::from(FilesystemErrorKind::NoEntry)),
        }
    }

    async fn read(
        &self,
        inode: u64,
        offset: u64,
        size: u64,
    ) -> Result<(Vec<u8>, bool), FilesystemError> {
        if inode != FILE_INODE {
            return Err(FilesystemError::from(FilesystemErrorKind::NotImplemented));
        }

        let file =
            std::fs::File::open(&self.file).map_err(|e| FilesystemErrorKind::IOError.with(e))?;
        let mut file = std::io::BufReader::new(file);

        use std::io::{Read, Seek};
        file.seek(std::io::SeekFrom::Start(offset))
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;
        let mut file = file.take(size);

        let mut data = Vec::new();
        let bytes_read = file
            .read_to_end(&mut data)
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;
        Ok((data, (bytes_read as u64) < size))
    }

    async fn readdir(
        &self,
        inode: u64,
        offset: u64,
        filler: &mut dyn ReadDirFiller,
    ) -> Result<bool, FilesystemError> {
        if offset >= 1 {
            return Ok(true);
        }

        if inode == FILE_INODE {
            return Err(FilesystemErrorKind::NotDirectory.into());
        }

        if inode != ROOT_INODE {
            return Err(FilesystemErrorKind::NoEntry.into());
        }

        // We are only ever going to add one entry, so we don't care about the return value
        let _ = filler.add(2, true, &self.filename);
        Ok(true)
    }

    async fn is_writeable(&self, _inode: u64) -> Result<bool, FilesystemError> {
        // FIXME: Support editing host fs
        Ok(false)
    }
}

#[async_trait]
impl Filesystem for HostDirectoryProviderInstance {
    async fn lookup(&self, parent: u64, filename: &str) -> Result<FileAttribute, FilesystemError> {
        let mut cache = self.inode_cache.write().expect("poisoned inode cache");
        let path = match parent {
            ROOT_INODE => self.root.join(filename),
            inode => cache
                .lookup_inode(inode)
                .ok_or(FilesystemErrorKind::NoEntry)?
                .join(filename),
        };

        let metadata = path
            .metadata()
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;
        let inode = cache.get_or_assign_inode(&path);
        Ok(metadata_to_attr(inode, &metadata))
    }

    async fn getattr(&self, inode: u64) -> Result<FileAttribute, FilesystemError> {
        if inode == ROOT_INODE {
            return Ok(metadata_to_attr(ROOT_INODE, &self.meta));
        }

        let cache = self.inode_cache.read().expect("poisoned inode cache");
        let metadata = cache
            .lookup_inode(inode)
            .ok_or(FilesystemErrorKind::NoEntry)?
            .metadata()
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;
        Ok(metadata_to_attr(inode, &metadata))
    }

    async fn read(
        &self,
        inode: u64,
        offset: u64,
        size: u64,
    ) -> Result<(Vec<u8>, bool), FilesystemError> {
        if inode == ROOT_INODE {
            return Err(FilesystemErrorKind::IsDirectory.into());
        }

        let cache = self.inode_cache.read().expect("poisoned inode cache");
        let path = cache
            .lookup_inode(inode)
            .ok_or(FilesystemErrorKind::NoEntry)?;
        let metadata = path
            .metadata()
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;
        if metadata.is_dir() {
            return Err(FilesystemError::from(FilesystemErrorKind::IsDirectory));
        }

        let file = std::fs::File::open(path).map_err(|e| FilesystemErrorKind::IOError.with(e))?;
        let mut file = std::io::BufReader::new(file);

        use std::io::{Read, Seek};
        file.seek(std::io::SeekFrom::Start(offset))
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;
        let mut file = file.take(size);

        log::info!("read from {path:?}");

        let mut data = Vec::new();
        let bytes_read = file
            .read_to_end(&mut data)
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;
        log::info!("read had {bytes_read} of {size}");
        Ok((data, (bytes_read as u64) < size))
    }

    async fn readdir(
        &self,
        inode: u64,
        offset: u64,
        filler: &mut dyn ReadDirFiller,
    ) -> Result<bool, FilesystemError> {
        let path = if inode == ROOT_INODE {
            self.root.clone()
        } else {
            let cache = self.inode_cache.read().expect("poisoned inode cache");
            let path = cache
                .lookup_inode(inode)
                .ok_or(FilesystemErrorKind::NoEntry)?;
            path.clone()
        };

        if !path.is_dir() {
            return Err(FilesystemErrorKind::NotDirectory.into());
        }

        let entries = path
            .read_dir()
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?
            .skip(offset as usize);

        for entry in entries {
            let entry = entry.map_err(|e| FilesystemErrorKind::IOError.with(e))?;
            let path = entry.path();
            let is_dir = entry
                .file_type()
                .map_err(|e| FilesystemErrorKind::IOError.with(e))?
                .is_dir();
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };

            let mut cache = self.inode_cache.write().expect("poisoned inode cache");
            let inode = cache.get_or_assign_inode(&path);

            if filler.add(inode, is_dir, name) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    async fn is_writeable(&self, _inode: u64) -> Result<bool, FilesystemError> {
        // FIXME: Support editing host fs
        Ok(false)
    }
}
