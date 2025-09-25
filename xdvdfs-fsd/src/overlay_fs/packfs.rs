use std::{
    collections::VecDeque,
    ops::DerefMut,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use tokio::sync::{Mutex, MutexGuard};
use xdvdfs::write::{
    fs::{
        FilesystemCopier, FilesystemHierarchy, SectorLinearBlockDevice,
        SectorLinearBlockFilesystem, SectorLinearImage, StdFilesystem,
    },
    img::ProgressInfo,
};

use crate::fsproto::{
    FileAttribute, Filesystem, FilesystemError, FilesystemErrorKind, ReadDirFiller,
};
use crate::hostutils::metadata_to_attr;

use super::{OverlayProvider, OverlayProviderInstance, ProviderInstance};

const ROOT_INODE: u64 = 1;
const IMAGE_INODE: u64 = 2;

struct PackOverlayProviderInstanceMutable<FS> {
    // Store duplicate name for debug logging.
    // It can't be reused by the provider because it is behind a lock, and
    // we want to avoid locking in those functions.
    name: String,
    slbfs: SectorLinearBlockFilesystem<FS>,
    slbd: SectorLinearBlockDevice,
}

type PackOverlayInstanceEntry<FS> = Mutex<PackOverlayProviderInstanceMutable<FS>>;

// FIXME: Replace with a better cache strategy
pub struct LifoCache<FS> {
    capacity: usize,
    live_entries: VecDeque<Arc<PackOverlayInstanceEntry<FS>>>,
}

impl<FS> Default for LifoCache<FS> {
    fn default() -> Self {
        Self {
            // TODO: Make this configurable
            capacity: 5,
            live_entries: VecDeque::new(),
        }
    }
}

impl<FS> LifoCache<FS> {
    /// Insert a new entry into the cache, evicting an existing entry if needed
    async fn insert(&mut self, entry: Arc<PackOverlayInstanceEntry<FS>>) {
        // If the new entry pushes us over capacity, pop the front entry
        if self.live_entries.len() + 1 > self.capacity {
            let front = self.live_entries.pop_front().unwrap();
            let mut front = front.lock().await;
            log::info!("evicting image {}", front.name);
            front.slbd.clear();
        }

        self.live_entries.push_back(entry);
    }
}

#[derive(Default)]
pub struct PackOverlayProvider {
    cache: Arc<Mutex<LifoCache<StdFilesystem>>>,
}

#[async_trait]
impl OverlayProvider for PackOverlayProvider {
    fn name(&self) -> &str {
        "pack overlay"
    }

    async fn matches_entry(&self, entry: &Path) -> Option<String> {
        // TODO: Match XGD images?
        if !entry.is_dir() {
            return None;
        }

        let xbe_path = entry.join("default.xbe");
        log::info!("xbe_path: {:?}", xbe_path);
        if !xbe_path.is_file() {
            return None;
        }

        let name = entry.file_name()?.to_str()?;
        Some(format!("{name}.xiso"))
    }

    async fn instantiate(
        &self,
        entry: &Path,
        name: &str,
    ) -> Result<ProviderInstance, FilesystemError> {
        // TODO: Handle XGD images?
        let fs = xdvdfs::write::fs::StdFilesystem::create(entry);
        let provider = PackOverlayProviderInstance::new(
            self.cache.clone(),
            fs,
            name.to_string(),
            entry.to_path_buf(),
        );

        log::info!("Instantiated pack provider for {name}");
        Ok(ProviderInstance::new(provider, 2, false))
    }
}

pub struct PackOverlayProviderInstance<FS> {
    inner: Arc<Mutex<PackOverlayProviderInstanceMutable<FS>>>,
    cache: Arc<Mutex<LifoCache<FS>>>,
    image_size: RwLock<u64>,
    image_name: String,
    src_path: PathBuf,
}

impl<FSHE, FCE, FS> PackOverlayProviderInstance<FS>
where
    FSHE: std::error::Error + 'static,
    FCE: std::error::Error + 'static,
    FS: FilesystemHierarchy<Error = FSHE> + FilesystemCopier<[u8], Error = FCE>,
{
    pub fn new(
        cache: Arc<Mutex<LifoCache<FS>>>,
        fs: FS,
        image_name: String,
        src_path: PathBuf,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(PackOverlayProviderInstanceMutable {
                name: image_name.clone(),
                slbfs: SectorLinearBlockFilesystem::new(fs),
                slbd: SectorLinearBlockDevice::default(),
            })),
            cache,

            // If the image is not present, assume it's the largest
            // possible size (~8GB). Packing the image here, or in `getattr`,
            // is prohibitively slow (e.g. it has to be done for every
            // image if `ls` is executed) so we defer it as long as possible.
            image_size: RwLock::new(8 * (1 << 30)),
            image_name,
            src_path,
        }
    }

    async fn pack(
        &self,
    ) -> Result<MutexGuard<'_, PackOverlayProviderInstanceMutable<FS>>, FilesystemError> {
        let mut data = self.inner.lock().await;
        if data.slbd.size() > 0 {
            return Ok(data);
        }

        let mut cache = self.cache.lock().await;
        cache.insert(self.inner.clone()).await;

        let src = self.src_path.display();
        let progress_callback = |pi: ProgressInfo<'_>| match pi {
            ProgressInfo::DirAdded(path, sector) => {
                log::trace!("Added dir: {src}{path} at sector {sector}");
            }
            ProgressInfo::FileAdded(path, sector) => {
                log::trace!("Added file: {src}{path} at sector {sector}");
            }
            _ => {}
        };

        log::info!("packing image {}", self.image_name);
        let data_mut = data.deref_mut();
        xdvdfs::write::img::create_xdvdfs_image(
            &mut data_mut.slbfs,
            &mut data_mut.slbd,
            progress_callback,
        )
        .await
        .map_err(|e| FilesystemErrorKind::IOError.with(e))?;

        let mut size = self.image_size.write().expect("poisoned size lock");
        *size = data_mut.slbd.size();

        Ok(data)
    }
}

#[async_trait]
impl<FSHE, FCE, FS> OverlayProviderInstance for PackOverlayProviderInstance<FS>
where
    FSHE: std::error::Error + 'static,
    FCE: std::error::Error + 'static,
    FS: FilesystemHierarchy<Error = FSHE> + FilesystemCopier<[u8], Error = FCE>,
{
}

#[async_trait]
impl<FSHE, FCE, FS> Filesystem for PackOverlayProviderInstance<FS>
where
    FSHE: std::error::Error + 'static,
    FCE: std::error::Error + 'static,
    FS: FilesystemHierarchy<Error = FSHE> + FilesystemCopier<[u8], Error = FCE>,
{
    async fn lookup(&self, parent: u64, filename: &str) -> Result<FileAttribute, FilesystemError> {
        if parent == IMAGE_INODE {
            return Err(FilesystemErrorKind::NotDirectory.into());
        }

        if parent != ROOT_INODE || filename != self.image_name {
            return Err(FilesystemErrorKind::NoEntry.into());
        }

        self.getattr(IMAGE_INODE).await
    }

    async fn getattr(&self, inode: u64) -> Result<FileAttribute, FilesystemError> {
        if inode != ROOT_INODE && inode != IMAGE_INODE {
            return Err(FilesystemErrorKind::NoEntry.into());
        }

        let metadata = self
            .src_path
            .metadata()
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;
        let mut attr = metadata_to_attr(inode, &metadata);

        // Overwrite these properties which depend on the type
        // of input. The output is always a RO file.
        attr.is_dir = inode == ROOT_INODE;
        attr.is_writeable = false;

        // Populate the size for the image
        if inode == IMAGE_INODE {
            let size = self.image_size.read().expect("poisoned size lock");
            attr.byte_size = *size;
            attr.block_size = 2048;
        }

        Ok(attr)
    }

    async fn read(
        &self,
        inode: u64,
        offset: u64,
        mut size: u64,
    ) -> Result<(Vec<u8>, bool), FilesystemError> {
        if inode == ROOT_INODE {
            return Err(FilesystemErrorKind::IsDirectory.into());
        }

        if inode != IMAGE_INODE {
            return Err(FilesystemErrorKind::NoEntry.into());
        }

        let mut data = self.pack().await?;
        let data = data.deref_mut();
        let image_size = data.slbd.size();

        // Since we might have given a fake size in getattr,
        // ensure any out of bounds reads are rejected or clamped
        if offset >= image_size && size != 0 {
            return Ok((Vec::new(), true));
        }

        if offset + size > image_size {
            size = image_size.saturating_sub(offset);
        }

        let mut img = SectorLinearImage::new(&data.slbd, &mut data.slbfs);

        let bytes = img
            .read_linear(offset, size)
            .await
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;
        let eof = offset + size >= image_size;
        Ok((bytes, eof))
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

        if inode == IMAGE_INODE {
            return Err(FilesystemErrorKind::NotDirectory.into());
        }

        if inode != ROOT_INODE {
            return Err(FilesystemErrorKind::NoEntry.into());
        }

        // We are only ever going to add one entry, so we don't care about the return value
        let _ = filler.add(IMAGE_INODE, true, &self.image_name);
        Ok(true)
    }

    async fn is_writeable(&self, inode: u64) -> Result<bool, FilesystemError> {
        match inode {
            ROOT_INODE | IMAGE_INODE => Ok(false),
            _ => Err(FilesystemErrorKind::NoEntry.into()),
        }
    }
}
