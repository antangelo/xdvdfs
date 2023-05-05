use std::path::{Path, PathBuf};

use alloc::{boxed::Box, vec::Vec};

use crate::blockdev::BlockDeviceWrite;

use async_trait::async_trait;

#[derive(Debug)]
pub enum FileType {
    File,
    Directory,
}

#[derive(Debug)]
pub struct FileEntry {
    pub path: PathBuf,
    pub file_type: FileType,
    pub len: u64,
}

pub struct DirectoryTreeEntry {
    pub dir: PathBuf,
    pub listing: Vec<FileEntry>,
}

#[async_trait(?Send)]
pub trait Filesystem<RawHandle: BlockDeviceWrite<E>, E> {
    /// Read a directory, and return a list of entries within it
    async fn read_dir(&self, path: &Path) -> Result<Vec<FileEntry>, E>;

    /// Copy the entire contents of file `src` into `dest` at the specified offset
    async fn copy_file_in(&self, src: &Path, dest: &mut RawHandle, offset: u64) -> Result<u64, E>;
}

#[cfg(not(target_family = "wasm"))]
pub struct StdFilesystem;

#[cfg(not(target_family = "wasm"))]
#[async_trait(?Send)]
impl Filesystem<std::fs::File, std::io::Error> for StdFilesystem {
    async fn read_dir(&self, dir: &Path) -> Result<Vec<FileEntry>, std::io::Error> {
        use std::fs::DirEntry;
        use std::io;

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

                    Ok(FileEntry {
                        path: de.path(),
                        file_type,
                        len: md.len(),
                    })
                })
            })
            .collect();

        let listing: io::Result<Vec<FileEntry>> = listing?.into_iter().collect();
        listing
    }

    async fn copy_file_in(
        &self,
        src: &Path,
        dest: &mut std::fs::File,
        offset: u64,
    ) -> Result<u64, std::io::Error> {
        use std::io::{Seek, SeekFrom};

        // FIXME: This is technically a race condition,
        // multiple threads could seek away from this position and corrupt the destination.
        // This needs a mutex to solve, but in practice isn't an issue
        // because create_xdvdfs_image copies files in sequentially.
        let file = std::fs::File::open(src)?;
        dest.seek(SeekFrom::Start(offset))?;
        let mut file = std::io::BufReader::with_capacity(1024 * 1024, file);
        std::io::copy(&mut file, dest)
    }
}
