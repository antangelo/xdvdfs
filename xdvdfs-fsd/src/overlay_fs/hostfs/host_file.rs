use std::{
    fs::Metadata,
    path::{Path, PathBuf},
};

use async_trait::async_trait;

use crate::{
    fsproto::{FileAttribute, Filesystem, FilesystemError, FilesystemErrorKind, ReadDirFiller},
    hostutils::metadata_to_attr,
    overlay_fs::ProviderInstance,
};

use super::{OverlayProviderInstance, FILE_INODE, ROOT_INODE};

pub struct HostFileProviderInstance {
    filename: String,
    file: PathBuf,
    attr: FileAttribute,
    offset: u64,
}

impl HostFileProviderInstance {
    pub fn new(name: &str, path: &Path) -> Result<Self, std::io::Error> {
        let meta = path.metadata()?;
        Ok(Self::new_with_metadata(name, path, &meta))
    }

    pub fn new_with_metadata(name: &str, path: &Path, meta: &Metadata) -> Self {
        Self {
            filename: name.to_string(),
            file: path.to_path_buf(),
            attr: metadata_to_attr(FILE_INODE, meta),
            offset: 0,
        }
    }

    pub fn set_offset(&mut self, offset: u64) {
        // Adjust the total size down by the offset amount, taking into account
        // any prior adjustments.
        self.attr.byte_size += self.offset;
        self.attr.byte_size -= offset;

        self.offset = offset;
    }
}

impl From<HostFileProviderInstance> for ProviderInstance {
    fn from(value: HostFileProviderInstance) -> Self {
        Self {
            filesystem: Box::new(value),
            inode: FILE_INODE,
            is_dir: false,
        }
    }
}

#[async_trait]
impl OverlayProviderInstance for HostFileProviderInstance {}

#[async_trait]
impl Filesystem for HostFileProviderInstance {
    async fn lookup(&self, parent: u64, filename: &str) -> Result<FileAttribute, FilesystemError> {
        if parent == ROOT_INODE && filename == self.filename {
            Ok(self.attr)
        } else {
            Err(FilesystemError::from(FilesystemErrorKind::NoEntry))
        }
    }

    async fn getattr(&self, inode: u64) -> Result<FileAttribute, FilesystemError> {
        match inode {
            FILE_INODE => Ok(self.attr),
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
        file.seek(std::io::SeekFrom::Start(offset + self.offset))
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
