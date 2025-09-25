use std::{
    fs::Metadata,
    path::{Path, PathBuf},
};

use async_trait::async_trait;

use crate::{
    fsproto::{FileAttribute, Filesystem, FilesystemError, FilesystemErrorKind, ReadDirFiller},
    hostutils::metadata_to_attr,
    inode::INodeCache,
    overlay_fs::ProviderInstance,
};

use super::{OverlayProviderInstance, ROOT_INODE};

pub struct HostDirectoryProviderInstance {
    root: PathBuf,
    root_attr: FileAttribute,
    inode_cache: std::sync::RwLock<INodeCache<PathBuf>>,
}

impl HostDirectoryProviderInstance {
    pub fn new(path: &Path) -> Result<Self, std::io::Error> {
        let meta = path.metadata()?;
        Ok(Self::new_with_metadata(path, &meta))
    }

    pub fn new_with_metadata(path: &Path, meta: &Metadata) -> Self {
        Self {
            root: path.to_path_buf(),
            root_attr: metadata_to_attr(ROOT_INODE, meta),
            inode_cache: std::sync::RwLock::new(INodeCache::default()),
        }
    }
}

impl From<HostDirectoryProviderInstance> for ProviderInstance {
    fn from(value: HostDirectoryProviderInstance) -> Self {
        Self {
            filesystem: Box::new(value),
            inode: ROOT_INODE,
            is_dir: true,
        }
    }
}

#[async_trait]
impl OverlayProviderInstance for HostDirectoryProviderInstance {}

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
            return Ok(self.root_attr);
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
