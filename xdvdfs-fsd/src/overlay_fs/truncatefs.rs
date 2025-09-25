use std::{fs::File, io::BufReader, path::Path};

use async_trait::async_trait;
use xdvdfs::blockdev::{OffsetWrapper, XDVDFSOffsets};

use crate::fsproto::{FilesystemError, FilesystemErrorKind};

use super::{hostfs::HostFileProviderInstance, OverlayProvider, ProviderInstance};

pub struct ImageTruncateFSFileProvider;

async fn get_image_offset(path: &Path) -> Option<XDVDFSOffsets> {
    let img = File::open(path).ok()?;
    let img = BufReader::new(img);
    let img = OffsetWrapper::new(img).await.ok()?;
    Some(img.get_offset())
}

#[async_trait]
impl OverlayProvider for ImageTruncateFSFileProvider {
    fn name(&self) -> &str {
        "xgd truncation"
    }

    async fn matches_entry(&self, entry: &Path) -> Option<String> {
        if !entry.is_file() {
            return None;
        }

        let extension = entry.extension()?;
        if extension != "iso" {
            return None;
        }

        // Only match XGD images, XISO images do not need truncation
        let offset = get_image_offset(entry).await?;
        if offset == XDVDFSOffsets::XISO {
            return None;
        }

        let name = entry
            .with_extension("xiso")
            .file_name()?
            .to_str()?
            .to_string();
        Some(name)
    }

    async fn instantiate(
        &self,
        entry: &Path,
        name: &str,
    ) -> Result<ProviderInstance, FilesystemError> {
        let offset = get_image_offset(entry)
            .await
            .ok_or(FilesystemErrorKind::IOError)?;
        let mut provider = HostFileProviderInstance::new(name, entry)
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;
        provider.set_offset(offset as u64);

        Ok(provider.into())
    }
}
