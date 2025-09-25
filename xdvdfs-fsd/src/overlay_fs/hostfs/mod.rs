use std::path::Path;

use async_trait::async_trait;

use crate::fsproto::{FilesystemError, FilesystemErrorKind};

use super::{OverlayProvider, OverlayProviderInstance, ProviderInstance};

const ROOT_INODE: u64 = 1;
const FILE_INODE: u64 = 2;

mod host_file;
pub use host_file::HostFileProviderInstance;

mod host_dir;
pub use host_dir::HostDirectoryProviderInstance;

/// File provider for overlayfs root nodes
/// Provides the host file for any file in the source directory (1-1 mapping)
pub struct OverlayFSFileProvider;

#[async_trait]
impl OverlayProvider for OverlayFSFileProvider {
    fn name(&self) -> &str {
        "host overlay"
    }

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

        let provider: ProviderInstance = if meta.is_dir() {
            HostDirectoryProviderInstance::new_with_metadata(entry, &meta).into()
        } else {
            HostFileProviderInstance::new_with_metadata(name, entry, &meta).into()
        };

        Ok(provider)
    }
}
