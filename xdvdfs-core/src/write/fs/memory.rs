use alloc::vec::Vec;
use maybe_async::maybe_async;

#[cfg(not(feature = "sync"))]
use alloc::boxed::Box;

use crate::{
    blockdev::NullBlockDevice,
    write::fs::{
        path::{PathPrefixTree, PathRef},
        FileEntry, FileType, FilesystemCopier, FilesystemHierarchy, PathVec,
    },
};

#[derive(Default, Debug, Clone)]
struct Entry {
    data: Option<Vec<u8>>,
}

#[derive(Default, Debug, Clone)]
pub struct MemoryFilesystem(PathPrefixTree<Entry>);

impl MemoryFilesystem {
    pub fn mkdir<'a, P: Into<PathRef<'a>>>(&mut self, path: P) {
        self.0.insert_path(path, Entry { data: None });
    }

    pub fn touch<'a, P: Into<PathRef<'a>>>(&mut self, path: P) {
        self.create(path, &[]);
    }

    pub fn create<'a, P: Into<PathRef<'a>>>(&mut self, path: P, data: &[u8]) {
        self.0.insert_path(
            path,
            Entry {
                data: Some(data.to_vec()),
            },
        );
    }

    pub fn lsdir<'a, P: Into<PathRef<'a>>>(&self, path: P) -> Option<Vec<FileEntry>> {
        let dir = self.0.lookup_subdir(path)?;
        let entries: Vec<FileEntry> = dir
            .iter()
            .map(|(name, entry)| FileEntry {
                name,
                file_type: match entry.data {
                    Some(_) => FileType::File,
                    None => FileType::Directory,
                },
                len: entry.data.as_ref().map(|d| d.len() as u64).unwrap_or(0),
            })
            .collect();
        Some(entries)
    }

    pub fn read<'a, P: Into<PathRef<'a>>>(
        &self,
        path: P,
        offset: usize,
        buffer: &mut [u8],
    ) -> Option<usize> {
        let file = self.0.get(path)?.data.as_ref()?;
        if offset >= file.len() {
            return None;
        }

        let size = core::cmp::min(buffer.len(), file.len() - offset);
        let limit = offset + size;
        assert!(limit <= file.len());

        buffer[0..size].copy_from_slice(&file[offset..limit]);
        Some(size)
    }

    // Split out impl into sync function for testing
    fn copy_file_in_impl<'a, P: Into<PathRef<'a>>>(
        &self,
        src: P,
        dest: &mut [u8],
        input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, ()> {
        let input_offset = input_offset as usize;
        let output_offset = output_offset as usize;

        if dest.len() <= output_offset {
            return Err(());
        }
        let limit = core::cmp::min(dest.len(), output_offset + (size as usize));
        assert!(limit >= output_offset);

        let buffer = &mut dest[output_offset..limit];
        let bytes_read = self.read(src, input_offset, buffer).ok_or(())?;
        dest[(output_offset + bytes_read)..limit].fill(0);
        Ok((limit - output_offset) as u64)
    }
}

#[maybe_async]
impl FilesystemHierarchy for MemoryFilesystem {
    type Error = ();

    async fn read_dir(&mut self, path: &PathVec) -> Result<Vec<FileEntry>, ()> {
        self.lsdir(path).ok_or(())
    }
}

#[maybe_async]
impl FilesystemCopier<[u8]> for MemoryFilesystem {
    type Error = ();

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut [u8],
        input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, ()> {
        self.copy_file_in_impl(src, dest, input_offset, output_offset, size)
    }
}

#[maybe_async]
impl FilesystemCopier<NullBlockDevice> for MemoryFilesystem {
    type Error = ();

    async fn copy_file_in(
        &mut self,
        _src: &PathVec,
        dest: &mut NullBlockDevice,
        _input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, ()> {
        dest.write_size_adjustment(output_offset, size);
        Ok(size)
    }
}

#[cfg(test)]
mod test {
    use alloc::string::ToString;

    use crate::write::fs::{FileEntry, FileType, MemoryFilesystem};

    #[test]
    fn test_memfs_lsdir_no_entry() {
        let memfs = MemoryFilesystem::default();
        assert_eq!(memfs.lsdir("/a"), None);
    }

    #[test]
    fn test_memfs_mkdir() {
        let mut memfs = MemoryFilesystem::default();
        memfs.mkdir("/a/b");
        memfs.mkdir("/a/c");

        let entries = memfs.lsdir("/a").expect("'/a' should contain records");
        assert_eq!(
            entries,
            [
                FileEntry {
                    name: "b".to_string(),
                    file_type: FileType::Directory,
                    len: 0,
                },
                FileEntry {
                    name: "c".to_string(),
                    file_type: FileType::Directory,
                    len: 0,
                },
            ]
        );
    }

    #[test]
    fn test_memfs_create() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &[0, 1, 2]);
        memfs.create("/a/c", &[3, 4]);

        let entries = memfs.lsdir("/a").expect("'/a' should contain records");
        assert_eq!(
            entries,
            [
                FileEntry {
                    name: "b".to_string(),
                    file_type: FileType::File,
                    len: 3,
                },
                FileEntry {
                    name: "c".to_string(),
                    file_type: FileType::File,
                    len: 2,
                },
            ]
        );
    }

    #[test]
    fn test_memfs_touch() {
        let mut memfs = MemoryFilesystem::default();
        memfs.touch("/a/b");
        memfs.touch("/a/c");

        let entries = memfs.lsdir("/a").expect("'/a' should contain records");
        assert_eq!(
            entries,
            [
                FileEntry {
                    name: "b".to_string(),
                    file_type: FileType::File,
                    len: 0,
                },
                FileEntry {
                    name: "c".to_string(),
                    file_type: FileType::File,
                    len: 0,
                },
            ]
        );
    }

    #[test]
    fn test_memfs_mixed_files_and_dirs() {
        let mut memfs = MemoryFilesystem::default();
        memfs.touch("/a/b");
        memfs.mkdir("/a/c");
        memfs.create("a/d", &[1]);

        let entries = memfs.lsdir("/a").expect("'/a' should contain records");
        assert_eq!(
            entries,
            [
                FileEntry {
                    name: "b".to_string(),
                    file_type: FileType::File,
                    len: 0,
                },
                FileEntry {
                    name: "c".to_string(),
                    file_type: FileType::Directory,
                    len: 0,
                },
                FileEntry {
                    name: "d".to_string(),
                    file_type: FileType::File,
                    len: 1,
                }
            ]
        );
    }

    #[test]
    fn test_memfs_read_no_entry() {
        let memfs = MemoryFilesystem::default();
        let mut buf = [0; 5];
        assert_eq!(memfs.read("/a", 0, &mut buf), None);
    }

    #[test]
    fn test_memfs_read_file_full() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &[1, 2, 3, 4, 5]);

        let mut buf = [0; 5];
        assert_eq!(memfs.read("/a/b", 0, &mut buf), Some(5));
        assert_eq!(buf, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_memfs_read_file_offset() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &[1, 2, 3, 4, 5]);

        let mut buf = [0; 5];
        assert_eq!(memfs.read("/a/b", 2, &mut buf), Some(3));
        assert_eq!(buf, [3, 4, 5, 0, 0]);
    }

    #[test]
    fn test_memfs_read_file_invalid_offset() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &[1, 2, 3, 4, 5]);

        let mut buf = [0; 5];
        assert_eq!(memfs.read("/a/b", 6, &mut buf), None);
    }

    #[test]
    fn test_memfs_read_directory_fails() {
        let mut memfs = MemoryFilesystem::default();
        memfs.mkdir("/a/b");

        let mut buf = [0; 5];
        assert_eq!(memfs.read("/a/b", 2, &mut buf), None);
    }

    #[test]
    fn test_memfs_copy_file_in() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &[1, 2, 3, 4, 5]);

        let mut buf = [0; 5];
        assert_eq!(memfs.copy_file_in_impl("/a/b", &mut buf, 0, 0, 5), Ok(5));
        assert_eq!(buf, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_memfs_copy_file_in_output_offset() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &[1, 2, 3, 4, 5]);

        // Read past end of file, extra space filled with 0
        let mut buf = [10; 9];
        assert_eq!(memfs.copy_file_in_impl("/a/b", &mut buf, 0, 2, 7), Ok(7));
        assert_eq!(buf, [10, 10, 1, 2, 3, 4, 5, 0, 0]);
    }

    #[test]
    fn test_memfs_copy_file_in_invalid_output_offset() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &[1, 2, 3, 4, 5]);

        let mut buf = [0; 5];
        assert_eq!(memfs.copy_file_in_impl("/a/b", &mut buf, 0, 6, 5), Err(()));
    }

    #[test]
    fn test_memfs_copy_file_in_buffer_size_clamped() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &[1, 2, 3, 4, 5]);

        // Output offset by 2, input offset by 0, data after EOF is filled 0
        let mut buf = [10; 8];
        assert_eq!(memfs.copy_file_in_impl("/a/b", &mut buf, 0, 2, 7), Ok(6));
        assert_eq!(buf, [10, 10, 1, 2, 3, 4, 5, 0]);
    }

    #[test]
    fn test_memfs_copy_file_in_input_offset() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &[1, 2, 3, 4, 5]);

        // Read is offset by 2, data after EOF is filled 0
        let mut buf = [1; 5];
        assert_eq!(memfs.copy_file_in_impl("/a/b", &mut buf, 2, 0, 5), Ok(5));
        assert_eq!(buf, [3, 4, 5, 0, 0]);
    }

    #[test]
    fn test_memfs_copy_file_in_invalid_input_offset() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &[1, 2, 3, 4, 5]);

        let mut buf = [0; 5];
        assert_eq!(memfs.copy_file_in_impl("/a/b", &mut buf, 5, 0, 5), Err(()));
    }
}
