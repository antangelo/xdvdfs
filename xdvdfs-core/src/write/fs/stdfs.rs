use alloc::borrow::ToOwned;
use alloc::vec::Vec;
use maybe_async::maybe_async;
use std::{
    fs::DirEntry,
    path::{Path, PathBuf},
};

#[cfg(not(feature = "sync"))]
use alloc::boxed::Box;

use crate::blockdev::{BlockDeviceWrite, NullBlockDevice};

use super::{FileEntry, FileType, FilesystemCopier, FilesystemHierarchy, PathRef};

pub struct StdFilesystem {
    root: PathBuf,
}

impl StdFilesystem {
    pub fn create(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
        }
    }

    fn direntry_to_file_entry(dir_entry: std::io::Result<DirEntry>) -> std::io::Result<FileEntry> {
        use std::io::{Error, ErrorKind};
        use std::string::ToString;

        let dir_entry = dir_entry?;
        let md = dir_entry.metadata()?;

        let file_type = if md.is_dir() {
            FileType::Directory
        } else if md.is_file() {
            FileType::File
        } else {
            return Err(Error::from(ErrorKind::Unsupported));
        };

        let name = dir_entry
            .path()
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .ok_or(Error::from(ErrorKind::Unsupported))?;

        Ok(FileEntry {
            name,
            file_type,
            len: md.len(),
        })
    }
}

#[maybe_async]
impl FilesystemHierarchy for StdFilesystem {
    type Error = std::io::Error;

    async fn read_dir(&mut self, dir: PathRef<'_>) -> Result<Vec<FileEntry>, std::io::Error> {
        let dir = dir.as_path_buf(&self.root);
        let listing: std::io::Result<Vec<FileEntry>> = std::fs::read_dir(dir)?
            .map(Self::direntry_to_file_entry)
            .collect();

        listing
    }
}

#[maybe_async]
impl<T> FilesystemCopier<T> for StdFilesystem
where
    T: std::io::Write + std::io::Seek + BlockDeviceWrite<WriteError = std::io::Error>,
{
    type Error = std::io::Error;

    async fn copy_file_in(
        &mut self,
        src: PathRef<'_>,
        dest: &mut T,
        input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, std::io::Error> {
        use std::io::{Read, Seek, SeekFrom};

        dest.seek(SeekFrom::Start(output_offset))?;

        let src = src.as_path_buf(&self.root);
        let mut file = std::fs::File::open(src)?;
        file.seek(SeekFrom::Start(input_offset))?;

        let mut file = std::io::BufReader::with_capacity(1024 * 1024, file.take(size));
        std::io::copy(&mut file, dest)
    }
}

#[maybe_async]
impl FilesystemCopier<alloc::boxed::Box<[u8]>> for StdFilesystem {
    type Error = std::io::Error;

    async fn copy_file_in(
        &mut self,
        src: PathRef<'_>,
        dest: &mut alloc::boxed::Box<[u8]>,
        input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, std::io::Error> {
        use std::io::{Read, Seek, SeekFrom};

        let src = src.as_path_buf(&self.root);
        let file = std::fs::File::open(src)?;
        let mut file = std::io::BufReader::with_capacity(1024 * 1024, file);
        file.seek(SeekFrom::Start(input_offset))?;

        let output_offset = output_offset as usize;
        let size = core::cmp::min(size as usize, <[u8]>::len(dest) - output_offset);

        let dest = &mut dest[output_offset..(output_offset + size)];
        let bytes_read = Read::read(&mut file, dest)?;
        dest[(output_offset + bytes_read)..].fill(0);
        Ok(<[u8]>::len(dest) as u64)
    }
}

#[maybe_async]
impl FilesystemCopier<[u8]> for StdFilesystem {
    type Error = std::io::Error;

    async fn copy_file_in(
        &mut self,
        src: PathRef<'_>,
        dest: &mut [u8],
        input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, std::io::Error> {
        use std::io::{Read, Seek, SeekFrom};

        let src = src.as_path_buf(&self.root);
        let file = std::fs::File::open(src)?;
        let mut file = std::io::BufReader::with_capacity(1024 * 1024, file);
        file.seek(SeekFrom::Start(input_offset))?;

        let output_offset = output_offset as usize;
        let size = core::cmp::min(size as usize, <[u8]>::len(dest) - output_offset);

        let dest = &mut dest[output_offset..(output_offset + size)];
        let bytes_read = Read::read(&mut file, dest)?;
        dest[(output_offset + bytes_read)..].fill(0);
        Ok(<[u8]>::len(dest) as u64)
    }
}

#[maybe_async]
impl FilesystemCopier<NullBlockDevice> for StdFilesystem {
    type Error = core::convert::Infallible;

    async fn copy_file_in(
        &mut self,
        _src: PathRef<'_>,
        dest: &mut NullBlockDevice,
        _input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, core::convert::Infallible> {
        dest.write_size_adjustment(output_offset, size);
        Ok(size)
    }
}
