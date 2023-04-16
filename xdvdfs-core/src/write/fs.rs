use std::fs::DirEntry;
use std::path::{Path, PathBuf};

use alloc::vec::Vec;

use crate::blockdev::BlockDeviceWrite;

pub enum FileType {
    File,
    Directory,
}

pub struct FileEntry {
    pub path: PathBuf,
    pub file_type: FileType,
    pub len: u64,
}

pub struct DirectoryTreeEntry {
    pub dir: PathBuf,
    pub listing: Vec<FileEntry>,
}

pub trait Filesystem<RawHandle: BlockDeviceWrite<E>, E> {
    /// Read a directory, and return a list of entries within it
    fn read_dir(&self, path: &Path) -> Result<Vec<FileEntry>, E>;

    /// Copy the entire contents of file `src` into `dest` at the specified offset
    fn copy_file_in(&self, src: &Path, dest: &mut RawHandle, offset: usize) -> Result<(), E>;
}

#[cfg(not(target_family = "wasm"))]
pub struct StdFilesystem;

#[cfg(not(target_family = "wasm"))]
impl Filesystem<std::fs::File, std::io::Error> for StdFilesystem {
    fn read_dir(&self, dir: &Path) -> Result<Vec<FileEntry>, std::io::Error> {
        let listing = std::fs::read_dir(&dir)?;
        let listing: std::io::Result<Vec<DirEntry>> = listing.collect();
        let listing: std::io::Result<Vec<FileEntry>> = listing?
            .into_iter()
            .map(|de| {
                de.metadata().map(|md| {
                    let file_type = if md.is_dir() {
                        FileType::Directory
                    } else if md.is_file() {
                        FileType::File
                    } else {
                        panic!("Invalid file type")
                    };

                    FileEntry {
                        path: de.path(),
                        file_type,
                        len: md.len(),
                    }
                })
            })
            .collect();
        listing
    }

    fn copy_file_in(
        &self,
        src: &Path,
        dest: &mut std::fs::File,
        offset: usize,
    ) -> Result<(), std::io::Error> {
        use std::io::{Seek, SeekFrom};
        let mut file = std::fs::File::open(src)?;
        dest.seek(SeekFrom::Start(offset as u64))?;
        std::io::copy(&mut file, dest)?;
        Ok(())
    }
}
