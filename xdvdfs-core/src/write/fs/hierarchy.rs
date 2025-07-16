use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use maybe_async::maybe_async;

use super::PathVec;

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
    pub listing: Vec<FileEntry>,
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
    let mut dirs = alloc::vec![PathVec::default()];
    let mut out = Vec::new();

    while let Some(dir) = dirs.pop() {
        let listing = fs.read_dir(&dir).await?;
        directory_found_callback(listing.len());

        for entry in listing.iter() {
            if let FileType::Directory = entry.file_type {
                dirs.push(PathVec::from_base(&dir, &entry.name));
            }
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
    async fn read_dir(&mut self, path: &PathVec) -> Result<Vec<FileEntry>, Self::Error>;

    /// Clear any cached data built during operation
    /// After clearing the cache, function should behave as though the object
    /// has just been initialized.
    async fn clear_cache(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Display a filesystem path as a String
    fn path_to_string(&self, path: &PathVec) -> String {
        path.as_string()
    }
}

#[maybe_async]
impl<E> FilesystemHierarchy for Box<dyn FilesystemHierarchy<Error = E>> {
    type Error = E;

    async fn read_dir(&mut self, path: &PathVec) -> Result<Vec<FileEntry>, E> {
        self.as_mut().read_dir(path).await
    }

    async fn clear_cache(&mut self) -> Result<(), Self::Error> {
        self.as_mut().clear_cache().await
    }

    fn path_to_string(&self, path: &PathVec) -> String {
        self.as_ref().path_to_string(path)
    }
}

#[maybe_async]
impl<E, F> FilesystemHierarchy for &mut F
where
    F: FilesystemHierarchy<Error = E> + ?Sized,
{
    type Error = E;

    async fn read_dir(&mut self, path: &PathVec) -> Result<Vec<FileEntry>, E> {
        (**self).read_dir(path).await
    }

    async fn clear_cache(&mut self) -> Result<(), Self::Error> {
        (**self).clear_cache().await
    }

    fn path_to_string(&self, path: &PathVec) -> String {
        (**self).path_to_string(path)
    }
}
