use std::{
    convert::Infallible,
    fs::{File, Metadata},
    io::BufReader,
    path::Path,
    time::SystemTime,
};

use xdvdfs::{
    blockdev::OffsetWrapper,
    layout::{DirectoryEntryTable, VolumeDescriptor},
};

use crate::{
    fsproto::{FileAttribute, FilesystemError, FilesystemErrorKind},
    inode::{INodeCache, INodeLookupResult},
};

pub struct ImageFilesystem {
    device: tokio::sync::Mutex<OffsetWrapper<BufReader<File>>>,
    src_atime: SystemTime,
    src_mtime: SystemTime,
    src_ctime: SystemTime,
    src_crtime: SystemTime,
    volume: VolumeDescriptor,
    cache: std::sync::RwLock<INodeCache>,
}

#[cfg(unix)]
fn file_ctime(metadata: &Metadata) -> SystemTime {
    use std::os::unix::fs::MetadataExt;
    use std::time::Duration;
    SystemTime::UNIX_EPOCH + Duration::new(metadata.ctime() as u64, metadata.ctime_nsec() as u32)
}

#[cfg(not(unix))]
fn file_ctime(metadata: &Metadata) -> SystemTime {
    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH)
}

impl ImageFilesystem {
    pub async fn new(img_path: &Path, metadata: &Metadata) -> anyhow::Result<ImageFilesystem> {
        let img = File::open(img_path)?;
        let img = BufReader::new(img);
        let mut device = OffsetWrapper::new(img).await?;

        // FIXME: Default ctime/crtime to image pack time, if available
        let src_atime = metadata.accessed().unwrap_or(SystemTime::UNIX_EPOCH);
        let src_mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let src_ctime = file_ctime(metadata);
        let src_crtime = metadata.created().unwrap_or(SystemTime::UNIX_EPOCH);

        let volume = xdvdfs::read::read_volume(&mut device).await?;

        Ok(Self {
            device: tokio::sync::Mutex::new(device),
            src_atime,
            src_mtime,
            src_ctime,
            src_crtime,
            volume,
            cache: std::sync::RwLock::new(INodeCache::new()),
        })
    }

    pub fn lookup_dirent_by_inode(&self, inode: u64) -> INodeLookupResult {
        if inode == 1 {
            return INodeLookupResult::RootEntry;
        }

        let cache = self.cache.read().expect("inode cache lock is poisoned");
        let inode = cache.lookup_inode(inode);
        match inode {
            Some(val) => INodeLookupResult::Value(*val),
            None => INodeLookupResult::NoEntry,
        }
    }

    pub fn lookup_dirtab_by_inode(&self, inode: u64) -> Option<DirectoryEntryTable> {
        if inode == 1 {
            Some(self.volume.root_table)
        } else {
            let cache = self.cache.read().expect("inode cache lock is poisoned");
            cache
                .lookup_inode(inode)
                .and_then(|dirent| dirent.node.dirent.dirent_table())
        }
    }

    fn new_file_attr(&self, inode: u64, byte_size: u64, is_dir: bool) -> FileAttribute {
        FileAttribute {
            inode,
            byte_size,
            block_size: xdvdfs::layout::SECTOR_SIZE as u64,
            is_dir,
            is_writeable: false,
            atime: self.src_atime,
            mtime: self.src_mtime,
            ctime: self.src_ctime,
            crtime: self.src_crtime,
        }
    }
}

impl crate::fsproto::Filesystem for ImageFilesystem {
    async fn lookup(&self, parent: u64, filename: &str) -> Result<FileAttribute, FilesystemError> {
        let dirtab = self
            .lookup_dirtab_by_inode(parent)
            .ok_or(FilesystemErrorKind::NotDirectory)?;

        let mut device = self.device.lock().await;
        let dirent = dirtab
            .walk_path(device.get_mut(), filename)
            .await
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;

        let mut cache = self.cache.write().expect("inode cache lock is poisoned");
        let inode = cache.get_or_assign_inode(&dirent);
        let file_attr = self.new_file_attr(
            inode,
            dirent.node.dirent.data.size as u64,
            dirent.node.dirent.is_directory(),
        );

        Ok(file_attr)
    }

    async fn getattr(&self, inode: u64) -> Result<FileAttribute, FilesystemError> {
        self.lookup_dirent_by_inode(inode)
            .some(
                |ent| {
                    self.new_file_attr(
                        inode,
                        ent.node.dirent.data.size.into(),
                        ent.node.dirent.is_directory(),
                    )
                },
                || self.new_file_attr(1, self.volume.root_table.region.size as u64, true),
            )
            .ok_or(FilesystemErrorKind::NoEntry.into())
    }

    async fn read(
        &self,
        inode: u64,
        offset: u64,
        size: u64,
    ) -> Result<(Vec<u8>, bool), FilesystemError> {
        let dirent = self.lookup_dirent_by_inode(inode);
        let dirent = match dirent {
            INodeLookupResult::Value(val) => Ok(val),
            INodeLookupResult::RootEntry => Err(FilesystemErrorKind::IsDirectory),
            INodeLookupResult::NoEntry => Err(FilesystemErrorKind::NoEntry),
        };
        let dirent = dirent?;

        let mut device = self.device.lock().await;
        let data = dirent
            .node
            .dirent
            .read_data_offset(device.get_mut(), size, offset)
            .await
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;
        let is_eof = data.len() as u64 != size;
        Ok((data.into_vec(), is_eof))
    }

    async fn readdir(
        &self,
        inode: u64,
        offset: u64,
        filler: &mut dyn crate::fsproto::ReadDirFiller,
    ) -> Result<bool, FilesystemError> {
        let dirtab = self
            .lookup_dirtab_by_inode(inode)
            .ok_or(FilesystemErrorKind::NotDirectory)?;

        let mut device = self.device.lock().await;
        let mut iter = dirtab
            .scan_dirent_tree(device.get_mut())
            .await
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;

        for _ in 0..offset {
            let next = iter
                .next_entry()
                .await
                .map_err(|e| FilesystemErrorKind::IOError.with(e))?;

            if next.is_none() {
                return Ok(true);
            }
        }

        loop {
            let next = iter
                .next_entry()
                .await
                .map_err(|e| FilesystemErrorKind::IOError.with(e))?;

            let Some(dirent) = next else {
                break;
            };

            let mut cache = self.cache.write().expect("inode cache is poisoned");
            let inode = cache.get_or_assign_inode(&dirent);
            let name = dirent.name_str::<Infallible>();
            let Ok(name) = name else {
                continue;
            };
            let name: String = name.to_string();
            let is_dir = dirent.node.dirent.is_directory();
            if filler.add(inode, is_dir, &name) {
                break;
            }
        }

        Ok(true)
    }

    async fn is_writeable(&self, _inode: u64) -> Result<bool, FilesystemError> {
        Ok(false)
    }
}
