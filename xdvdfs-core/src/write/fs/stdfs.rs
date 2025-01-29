use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use maybe_async::maybe_async;
use std::path::{Path, PathBuf};

#[cfg(not(feature = "sync"))]
use alloc::boxed::Box;

use crate::blockdev::BlockDeviceWrite;

use super::{FileEntry, FileType, FilesystemCopier, FilesystemHierarchy, PathVec};

pub struct StdFilesystem {
    root: PathBuf,
}

impl StdFilesystem {
    pub fn create(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
        }
    }
}

#[maybe_async]
impl FilesystemHierarchy for StdFilesystem {
    type Error = std::io::Error;

    async fn read_dir(&mut self, dir: &PathVec) -> Result<Vec<FileEntry>, std::io::Error> {
        use alloc::string::ToString;
        use std::fs::DirEntry;
        use std::io;

        let dir = dir.as_path_buf(&self.root);
        let listing: io::Result<Vec<DirEntry>> = std::fs::read_dir(dir)?.collect();
        let listing: io::Result<Vec<io::Result<FileEntry>>> = listing?
            .into_iter()
            .map(|de| {
                de.metadata().map(|md| {
                    let file_type = if md.is_dir() {
                        FileType::Directory
                    } else if md.is_file() {
                        FileType::File
                    } else {
                        return Err(io::Error::from(io::ErrorKind::Unsupported));
                    };

                    let name = de
                        .path()
                        .file_name()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_string())
                        .ok_or(io::Error::from(io::ErrorKind::Unsupported))?;

                    Ok(FileEntry {
                        name,
                        file_type,
                        len: md.len(),
                    })
                })
            })
            .collect();

        let listing: io::Result<Vec<FileEntry>> = listing?.into_iter().collect();
        listing
    }

    fn path_to_string(&self, path: &PathVec) -> String {
        let path = path.as_path_buf(&self.root);
        format!("{:?}", path)
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
        src: &PathVec,
        dest: &mut T,
        offset: u64,
        _size: u64,
    ) -> Result<u64, std::io::Error> {
        use std::io::SeekFrom;

        dest.seek(SeekFrom::Start(offset))?;

        let src = src.as_path_buf(&self.root);
        let file = std::fs::File::open(src)?;
        let mut file = std::io::BufReader::with_capacity(1024 * 1024, file);
        std::io::copy(&mut file, dest)
    }
}

#[maybe_async]
impl FilesystemCopier<Box<[u8]>> for StdFilesystem {
    type Error = std::io::Error;

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut Box<[u8]>,
        offset: u64,
        _size: u64,
    ) -> Result<u64, std::io::Error> {
        use std::io::{Read, Seek, SeekFrom};

        let src = src.as_path_buf(&self.root);
        let file = std::fs::File::open(src)?;
        let mut file = std::io::BufReader::with_capacity(1024 * 1024, file);
        file.seek(SeekFrom::Start(offset))?;

        let bytes_read = Read::read(&mut file, dest)?;
        dest[bytes_read..].fill(0);
        Ok(<[u8]>::len(dest) as u64)
    }
}

#[maybe_async]
impl<'a> FilesystemCopier<&'a mut [u8]> for StdFilesystem {
    type Error = std::io::Error;

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut &'a mut [u8],
        offset: u64,
        _size: u64,
    ) -> Result<u64, std::io::Error> {
        use std::io::{Read, Seek, SeekFrom};

        let src = src.as_path_buf(&self.root);
        let file = std::fs::File::open(src)?;
        let mut file = std::io::BufReader::with_capacity(1024 * 1024, file);
        file.seek(SeekFrom::Start(offset))?;

        let bytes_read = Read::read(&mut file, dest)?;
        dest[bytes_read..].fill(0);
        Ok(<[u8]>::len(dest) as u64)
    }
}
