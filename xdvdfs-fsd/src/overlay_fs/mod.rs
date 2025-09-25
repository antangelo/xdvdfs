use std::{
    collections::{BTreeMap, HashSet},
    ffi::OsString,
    fs::Metadata,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::fsproto::{
    FileAttribute, Filesystem, FilesystemError, FilesystemErrorKind, ReadDirFiller,
};
use crate::hostutils::metadata_to_attr;

pub mod hostfs;

#[cfg(not(feature = "sync"))]
pub mod packfs;

#[cfg(not(feature = "sync"))]
pub mod truncatefs;

pub struct OverlayFSBuilder {
    src: PathBuf,
    providers: Vec<Box<dyn OverlayProvider + Send + Sync>>,
}

/// An abstract overlay filesystem, that sits on top of an original directory
/// and provides overlay files.
///
/// A set of OverlayProviders match against top-level entries in the source,
/// and provide file system records from the underlying data.
///
/// TODO: Allow mutation of underlying files
pub struct OverlayFS {
    src: PathBuf,
    src_meta: Metadata,

    // The first element is always the OverlayFS provider
    providers: Vec<Box<dyn OverlayProvider + Send + Sync>>,

    state: RwLock<OverlayFSMutableState>,
}

#[derive(Default)]
struct OverlayFSMutableState {
    // Instances for individual provided files
    provider_instances: Vec<Option<ProviderInstance>>,

    // Map of provided entries (in the root) to their instances
    provided_entry_instance_map: BTreeMap<String, ProvidedEntry>,

    // Map of host entries to their provider instances (if any)
    // Used for mutation tracking (TODO)
    host_entry_instance_map: BTreeMap<OsString, HashSet<usize>>,
}

struct ProvidedEntry {
    provider_instance_idx: usize,
}

pub struct ProviderInstance {
    filesystem: Box<dyn OverlayProviderInstance + Send + Sync>,
    inode: u64,
    is_dir: bool,
}

#[async_trait]
pub trait OverlayProvider {
    /// Returns a display name for the provider, for logging
    /// Should be readable in context like
    /// "instantiated <name()> provider for <filename>"
    fn name(&self) -> &str {
        "unknown"
    }

    /// Determine if a provider is interested in matching a record.
    /// If a provider is interested, it should return `Some(name)`,
    /// where `name` is the name of the entry provided in the overlay root
    /// by this provider.
    /// If it is not interested, it should return `None`.
    ///
    /// Ideally, this function is as inexpensive as possible.
    /// It is possible that a higher-priority provider will register
    /// the same file name, and only that provider will respond when
    /// filesystem queries are made on that file.
    async fn matches_entry(&self, entry: &Path) -> Option<String>;

    /// For a given matched entry, instantiate the underlying provider
    /// The provider is given as a Filesystem with some inode corresponding
    /// to the entry.
    async fn instantiate(
        &self,
        entry: &Path,
        name: &str,
    ) -> Result<ProviderInstance, FilesystemError>;
}

#[async_trait]
pub trait OverlayProviderInstance: Filesystem {}

impl ProviderInstance {
    pub fn new<I: OverlayProviderInstance + Send + Sync + 'static>(
        filesystem: I,
        entry_inode: u64,
        is_dir: bool,
    ) -> Self {
        Self {
            filesystem: Box::new(filesystem),
            inode: entry_inode,
            is_dir,
        }
    }
}

impl OverlayFSBuilder {
    pub fn new<P: AsRef<Path>>(src: P) -> Self {
        Self {
            src: src.as_ref().to_path_buf(),
            providers: vec![Box::new(hostfs::OverlayFSFileProvider)],
        }
    }

    pub fn with_provider<P: OverlayProvider + Send + Sync + 'static>(
        mut self,
        provider: P,
    ) -> Self {
        self.providers.push(Box::new(provider));
        self
    }

    pub fn build(self) -> std::io::Result<OverlayFS> {
        let src_meta = self.src.metadata()?;
        Ok(OverlayFS {
            src: self.src,
            src_meta,
            providers: self.providers,
            state: RwLock::new(OverlayFSMutableState::default()),
        })
    }
}

impl OverlayFS {
    // Map a provider inode (by provider instance index) to an overlayfs inode
    fn inode_mask(provider_instance: usize, provider_inode: u64) -> u64 {
        assert_eq!(provider_inode >> 48, 0);
        ((provider_instance as u64) << 48) | provider_inode
    }

    // Map an overlayfs inode to a provider inode (by provider index)
    fn inode_unmask(overlayfs_inode: u64) -> u64 {
        overlayfs_inode & 0x0000_ffff_ffff_ffff
    }

    // Returns the provider index of an overlayfs inode
    fn inode_provider_instance(overlayfs_inode: u64) -> usize {
        (overlayfs_inode >> 48) as usize
    }

    // Scan the source path and instantiate any new providers
    async fn scan(&self) -> Result<(), FilesystemError> {
        let mut state = self.state.write().await;

        let entries = self
            .src
            .read_dir()
            .map_err(|e| FilesystemErrorKind::IOError.with(e))?;
        for entry in entries {
            let entry = entry.map_err(|e| FilesystemErrorKind::IOError.with(e))?;
            let path = entry.path();
            let file_name = entry.file_name();

            // Providers are immutable, and the host FS provider will always
            // map root entries to themselves, so if we have seen the host file already,
            // we do not need to rescan.
            if state.host_entry_instance_map.contains_key(&file_name) {
                continue;
            }

            for provider in &self.providers {
                let record = provider.matches_entry(&path).await;
                let record = match record {
                    Some(record) => record,
                    None => continue,
                };

                // Providers are indexed starting at 1
                let next_instance_idx = state.provider_instances.len() + 1;

                let map_entry = state.provided_entry_instance_map.entry(record.clone());
                use std::collections::btree_map::Entry;
                match map_entry {
                    Entry::Vacant(v) => {
                        let instance = provider.instantiate(&path, &record).await?;
                        log::info!(
                            "instantiated {} provider for record {}",
                            provider.name(),
                            record,
                        );
                        v.insert(ProvidedEntry {
                            provider_instance_idx: next_instance_idx,
                        });
                        state
                            .host_entry_instance_map
                            .entry(file_name.clone())
                            .or_default()
                            .insert(next_instance_idx);
                        state.provider_instances.push(Some(instance));
                    }
                    Entry::Occupied(_) => continue,
                }
            }
        }

        Ok(())
    }

    // Handle readdir called on the root inode (inode 1)
    // This requires access to the providers, so it cannot
    // be handled in the OverlayFS provider
    async fn root_readdir(
        &self,
        inode: u64,
        offset: u64,
        filler: &mut dyn ReadDirFiller,
    ) -> Result<bool, FilesystemError> {
        if inode != 1 {
            return Err(FilesystemError::from(FilesystemErrorKind::NoEntry));
        }

        self.scan().await?;
        let state = self.state.read().await;
        let entries = state
            .provided_entry_instance_map
            .iter()
            .skip(offset as usize);
        for (name, entry) in entries {
            let provider = state.get_provider_instance(entry.provider_instance_idx);
            let Some(provider) = provider else {
                continue;
            };

            let inode = Self::inode_mask(entry.provider_instance_idx, provider.inode);
            if filler.add(inode, provider.is_dir, name) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    async fn root_lookup(
        &self,
        parent: u64,
        filename: &str,
    ) -> Result<FileAttribute, FilesystemError> {
        if parent != 1 {
            return Err(FilesystemError::from(FilesystemErrorKind::NoEntry));
        }

        self.scan().await?;
        let state = self.state.read().await;
        let provider_entry = state
            .provided_entry_instance_map
            .get(filename)
            .ok_or(FilesystemErrorKind::NoEntry)?;
        let provider = state
            .get_provider_instance(provider_entry.provider_instance_idx)
            .ok_or(FilesystemErrorKind::NoEntry)?;
        let mut attr = provider.filesystem.getattr(provider.inode).await?;
        attr.inode = Self::inode_mask(provider_entry.provider_instance_idx, provider.inode);
        Ok(attr)
    }
}

impl OverlayFSMutableState {
    fn get_provider_instance(&self, provider_instance: usize) -> Option<&ProviderInstance> {
        if provider_instance == 0 {
            return None;
        }

        self.provider_instances
            .get(provider_instance - 1)
            .and_then(|x| x.as_ref())
    }
}

struct OverlayFSMappingDirFiller<'a> {
    provider_idx: usize,
    filler: &'a mut dyn ReadDirFiller,
}

impl ReadDirFiller for OverlayFSMappingDirFiller<'_> {
    fn add(&mut self, inode: u64, is_dir: bool, name: &str) -> bool {
        let inode = OverlayFS::inode_mask(self.provider_idx, inode);
        self.filler.add(inode, is_dir, name)
    }
}

#[async_trait]
impl Filesystem for OverlayFS {
    async fn lookup(&self, parent: u64, filename: &str) -> Result<FileAttribute, FilesystemError> {
        let provider_idx = Self::inode_provider_instance(parent);
        let provider_inode = Self::inode_unmask(parent);

        if provider_idx == 0 {
            return self.root_lookup(provider_inode, filename).await;
        }

        let state = self.state.read().await;
        let provider = state
            .get_provider_instance(provider_idx)
            .ok_or(FilesystemErrorKind::NoEntry)?;

        let mut attr = provider.filesystem.lookup(provider_inode, filename).await?;
        attr.inode = Self::inode_mask(provider_idx, attr.inode);
        Ok(attr)
    }

    async fn getattr(&self, inode: u64) -> Result<FileAttribute, FilesystemError> {
        let provider_idx = Self::inode_provider_instance(inode);
        let provider_inode = Self::inode_unmask(inode);

        if provider_idx == 0 {
            if provider_inode != 1 {
                return Err(FilesystemErrorKind::NoEntry.into());
            }

            let attr = metadata_to_attr(1, &self.src_meta);
            return Ok(attr);
        }

        let state = self.state.read().await;
        let provider = state
            .get_provider_instance(provider_idx)
            .ok_or(FilesystemErrorKind::NoEntry)?;

        let mut attr = provider.filesystem.getattr(provider_inode).await?;
        attr.inode = Self::inode_mask(provider_idx, attr.inode);
        Ok(attr)
    }

    async fn read(
        &self,
        inode: u64,
        offset: u64,
        size: u64,
    ) -> Result<(Vec<u8>, bool), FilesystemError> {
        let provider_idx = Self::inode_provider_instance(inode);
        let provider_inode = Self::inode_unmask(inode);

        if provider_idx == 0 {
            return Err(match provider_inode {
                1 => FilesystemErrorKind::IsDirectory,
                _ => FilesystemErrorKind::NoEntry,
            }
            .into());
        }

        let state = self.state.read().await;
        let provider = state
            .get_provider_instance(provider_idx)
            .ok_or(FilesystemErrorKind::NoEntry)?;
        provider.filesystem.read(provider_inode, offset, size).await
    }

    async fn readdir(
        &self,
        inode: u64,
        offset: u64,
        filler: &mut dyn ReadDirFiller,
    ) -> Result<bool, FilesystemError> {
        let provider_idx = Self::inode_provider_instance(inode);
        let provider_inode = Self::inode_unmask(inode);

        if provider_idx == 0 {
            return self.root_readdir(provider_inode, offset, filler).await;
        }

        let state = self.state.read().await;
        let provider = state
            .get_provider_instance(provider_idx)
            .ok_or(FilesystemErrorKind::NoEntry)?;

        let mut overlayfs_filler = OverlayFSMappingDirFiller {
            provider_idx,
            filler,
        };
        provider
            .filesystem
            .readdir(provider_inode, offset, &mut overlayfs_filler)
            .await
    }

    async fn is_writeable(&self, inode: u64) -> Result<bool, FilesystemError> {
        let provider_idx = Self::inode_provider_instance(inode);
        let provider_inode = Self::inode_unmask(inode);

        if provider_idx == 0 {
            return Ok(false);
        }

        let state = self.state.read().await;
        let provider = state
            .get_provider_instance(provider_idx)
            .ok_or(FilesystemErrorKind::NoEntry)?;
        provider.filesystem.is_writeable(provider_inode).await
    }
}
