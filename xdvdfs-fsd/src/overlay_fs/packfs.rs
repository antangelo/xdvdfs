use std::{
    ops::DerefMut,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use tokio::sync::Mutex;
use xdvdfs::write::{
    fs::{
        FilesystemCopier, FilesystemHierarchy, SectorLinearBlockDevice,
        SectorLinearBlockFilesystem, SectorLinearImage,
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

pub struct PackOverlayProvider;

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
        let provider = PackOverlayProviderInstance::new(fs, name.to_string(), entry.to_path_buf());

        log::info!("Instantiated pack provider for {name}");
        Ok(ProviderInstance::new(provider, 2, false))
    }
}

struct PackOverlayProviderInstanceMutable<FS> {
    slbfs: SectorLinearBlockFilesystem<FS>,
    slbd: SectorLinearBlockDevice,
}

pub struct PackOverlayProviderInstance<FS> {
    inner: Mutex<PackOverlayProviderInstanceMutable<FS>>,
    image_name: String,
    src_path: PathBuf,
}

impl<FSHE, FCE, FS> PackOverlayProviderInstance<FS>
where
    FSHE: std::error::Error + 'static,
    FCE: std::error::Error + 'static,
    FS: FilesystemHierarchy<Error = FSHE> + FilesystemCopier<[u8], Error = FCE>,
{
    pub fn new(fs: FS, image_name: String, src_path: PathBuf) -> Self {
        Self {
            inner: Mutex::new(PackOverlayProviderInstanceMutable {
                slbfs: SectorLinearBlockFilesystem::new(fs),
                slbd: SectorLinearBlockDevice::default(),
            }),
            image_name,
            src_path,
        }
    }

    async fn pack(&self) -> Result<(), FilesystemError> {
        let mut data = self.inner.lock().await;
        if data.slbd.size() > 0 {
            return Ok(());
        }

        let src = self.src_path.display();
        let progress_callback = |pi: ProgressInfo<'_>| match pi {
            ProgressInfo::DirAdded(path, sector) => {
                log::info!("Added dir: {src}{path} at sector {sector}");
            }
            ProgressInfo::FileAdded(path, sector) => {
                log::info!("Added file: {src}{path} at sector {sector}");
            }
            _ => {}
        };

        log::info!("Packing image {}", self.image_name);
        let data = data.deref_mut();
        xdvdfs::write::img::create_xdvdfs_image(&mut data.slbfs, &mut data.slbd, progress_callback)
            .await
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;

        Ok(())
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

        // Populate the correct size for the image
        if inode == IMAGE_INODE {
            let data = self.inner.lock().await;
            let size = data.slbd.size();

            // If the image is not present, assume it's the largest
            // possible size (~8GB). Packing the image in this function
            // is prohibitively slow (e.g. it has to be done for every
            // image if `ls` is executed)
            attr.byte_size = match size {
                0 => 8 * (1 << 30),
                sz => sz,
            };

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

        self.pack().await?;

        let mut data = self.inner.lock().await;
        let data = data.deref_mut();
        let image_size = data.slbd.size();

        // Since we might have given a fake size in getattr,
        // ensure any out of bounds reads are rejected or clamped
        if offset >= image_size && size != 0 {
            return Ok((Vec::new(), true));
        }

        if offset + size > image_size {
            size = image_size - offset;
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
