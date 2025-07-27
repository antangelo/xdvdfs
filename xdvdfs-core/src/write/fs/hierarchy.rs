use std::collections::VecDeque;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use maybe_async::maybe_async;

use super::{PathRef, PathVec};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileType {
    File,
    Directory,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileEntry {
    pub name: String,
    pub file_type: FileType,
    pub len: u64,
}

#[derive(Clone, Debug)]
pub struct DirectoryTreeEntry {
    pub dir: PathVec,
    pub listing: Vec<(FileEntry, usize)>,
}

/// Returns a recursive listing of paths in order
/// e.g. for a path hierarchy like this:
/// /
/// -- /a
/// -- -- /a/b
/// -- /b
/// It might return the list: ["/", "/a", "/a/b", "/b"]
/// `directory_found_callback` should be called each time
/// a new directory is found, with the number of entries
/// in that directory (for progress tracking).
#[maybe_async]
pub async fn dir_tree<FS: FilesystemHierarchy + ?Sized>(
    fs: &mut FS,
    directory_found_callback: &mut impl FnMut(usize),
) -> Result<Vec<DirectoryTreeEntry>, FS::Error> {
    let mut dirs: VecDeque<PathVec> = VecDeque::new();
    dirs.push_back(PathVec::default());
    let mut out = Vec::new();

    while let Some(dir) = dirs.pop_front() {
        let entries = fs.read_dir(dir.as_path_ref()).await?;
        directory_found_callback(entries.len());
        let mut listing: Vec<(FileEntry, usize)> = Vec::with_capacity(entries.len());

        let current_dir_index = out.len();

        for entry in entries.into_iter() {
            let mut dir_index: usize = 0;
            if let FileType::Directory = entry.file_type {
                dirs.push_back(PathVec::from_base(dir.clone(), &entry.name));

                // This directory is in position `dirs.len()` in the queue,
                // so it will be that many indices past the current directory.
                dir_index = current_dir_index + dirs.len();
            }

            listing.push((entry, dir_index));
        }

        out.push(DirectoryTreeEntry { dir, listing });
    }

    Ok(out)
}

/// A trait for filesystem hierarchies, representing any filesystem
/// structure with hierarchical directories.
///
/// This trait allows for scanning a given directory within a filesystem
/// for a list of its entries and entry metadata.
#[maybe_async]
pub trait FilesystemHierarchy: Send + Sync {
    type Error;

    /// Read a directory, and return a list of entries within it
    async fn read_dir(&mut self, path: PathRef<'_>) -> Result<Vec<FileEntry>, Self::Error>;

    /// Clear any cached data built during operation
    /// After clearing the cache, function should behave as though the object
    /// has just been initialized.
    async fn clear_cache(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[maybe_async]
impl<E> FilesystemHierarchy for Box<dyn FilesystemHierarchy<Error = E>> {
    type Error = E;

    async fn read_dir(&mut self, path: PathRef<'_>) -> Result<Vec<FileEntry>, E> {
        self.as_mut().read_dir(path).await
    }

    async fn clear_cache(&mut self) -> Result<(), Self::Error> {
        self.as_mut().clear_cache().await
    }
}

#[maybe_async]
impl<E, F> FilesystemHierarchy for &mut F
where
    F: FilesystemHierarchy<Error = E> + ?Sized,
{
    type Error = E;

    async fn read_dir(&mut self, path: PathRef<'_>) -> Result<Vec<FileEntry>, E> {
        (**self).read_dir(path).await
    }

    async fn clear_cache(&mut self) -> Result<(), Self::Error> {
        (**self).clear_cache().await
    }
}
