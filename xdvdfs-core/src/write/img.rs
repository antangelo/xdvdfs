use alloc::collections::BTreeMap;
use alloc::string::String;

use crate::blockdev::BlockDeviceWrite;
use crate::layout;
use crate::util::FileTime;
use crate::write::{fs, sector};

use alloc::vec;

use super::dirtab::{
    AvlDirectoryEntryTableBuilder, DirectoryEntryTableBuilder, DirectoryEntryTableWriter,
};
use super::fs::{DirectoryTreeEntry, FilesystemCopier, FilesystemHierarchy, PathVec};
use super::{FileStructureError, WriteError};

use maybe_async::maybe_async;

#[non_exhaustive]
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProgressInfo {
    DiscoveredDirectory(usize),
    FileCount(usize),
    DirCount(usize),
    DirAdded(String, u64),
    FileAdded(String, u64),
    FinishedCopyingImageData,
    FinishedPacking,
}

type DirentTableMap<'a, DTW> = BTreeMap<&'a PathVec, DTW>;

fn create_dirent_tables<'a, DTB: DirectoryEntryTableBuilder>(
    dirtree: &'a [DirectoryTreeEntry],
) -> Result<(DirentTableMap<'a, DTB::DirtabWriter>, usize), FileStructureError> {
    let mut dirent_tables: DirentTableMap<'_, DTB::DirtabWriter> = BTreeMap::new();
    let mut count = 0;

    for entry in dirtree.iter().rev() {
        let path = &entry.dir;
        let dir_entries = &entry.listing;

        let mut dirtab = DTB::default();

        for entry in dir_entries {
            let file_name = entry.name.as_str();
            count += 1;

            match entry.file_type {
                fs::FileType::Directory => {
                    // TODO: Replace with PathRef::join
                    let entry_path = PathVec::from_base(path, file_name);
                    let dir_size = dirent_tables
                        .get(&entry_path)
                        .expect("path should have been computed in previous iteration")
                        .dirtab_size();
                    dirtab.add_dir(file_name, dir_size)?;
                }
                fs::FileType::File => {
                    let file_size = entry
                        .len
                        .try_into()
                        .map_err(|_| FileStructureError::FileTooLarge)?;
                    dirtab.add_file(file_name, file_size)?;
                }
            }
        }

        dirent_tables.insert(path, dirtab.build()?);
    }

    Ok((dirent_tables, count))
}

type GenericWriteError<BDW, FS> = WriteError<
    <BDW as BlockDeviceWrite>::WriteError,
    <FS as FilesystemHierarchy>::Error,
    <FS as FilesystemCopier<BDW>>::Error,
>;

#[maybe_async]
async fn write_volume_descriptor<
    BDW: BlockDeviceWrite + ?Sized,
    FS: FilesystemHierarchy + FilesystemCopier<BDW> + ?Sized,
>(
    image: &mut BDW,
    filetime: FileTime,
    root_dirtab_size: u32,
    root_dirtab_sector: u32,
) -> Result<(), GenericWriteError<BDW, FS>> {
    // FIXME: Set timestamp
    let root_table = layout::DirectoryEntryTable::new(root_dirtab_size, root_dirtab_sector);
    let volume_info = layout::VolumeDescriptor::with_filetime(root_table, filetime);
    let volume_info = volume_info
        .serialize()
        .map_err(|e| FileStructureError::SerializationError(e.into()))?;
    image
        .write(32 * layout::SECTOR_SIZE as u64, &volume_info)
        .await
        .map_err(WriteError::BlockDeviceError)?;
    Ok(())
}

#[maybe_async]
async fn apply_image_alignment_padding<
    BDW: BlockDeviceWrite + ?Sized,
    FS: FilesystemHierarchy + FilesystemCopier<BDW> + ?Sized,
>(
    image: &mut BDW,
) -> Result<(), GenericWriteError<BDW, FS>> {
    let len = image.len().await.map_err(WriteError::BlockDeviceError)?;

    let aligned = len.next_multiple_of(32 * layout::SECTOR_SIZE as u64);
    let padding = aligned - len;
    let padding: usize = padding.try_into().expect("padding < 32 * SECTOR_SIZE");

    if padding > 0 {
        let padding = vec![0x00; padding];
        image
            .write(len, &padding)
            .await
            .map_err(WriteError::BlockDeviceError)?;
    }

    Ok(())
}

#[maybe_async]
pub async fn create_xdvdfs_image<
    BDW: BlockDeviceWrite + ?Sized,
    FS: FilesystemHierarchy + FilesystemCopier<BDW> + ?Sized,
>(
    fs: &mut FS,
    image: &mut BDW,
    mut progress_callback: impl FnMut(ProgressInfo),
) -> Result<(), GenericWriteError<BDW, FS>> {
    create_xdvdfs_image_with_filetime(fs, image, FileTime::default(), &mut progress_callback).await
}

#[maybe_async]
pub async fn create_xdvdfs_image_with_filetime<
    BDW: BlockDeviceWrite + ?Sized,
    FS: FilesystemHierarchy + FilesystemCopier<BDW> + ?Sized,
>(
    fs: &mut FS,
    image: &mut BDW,
    filetime: FileTime,
    mut progress_callback: impl FnMut(ProgressInfo),
) -> Result<(), GenericWriteError<BDW, FS>> {
    // The size of a directory entry depends on the size of
    // directory entries inside it (including directory tables).
    // We need to compute the size of all dirent tables before
    // writing the image. As such, we iterate over a directory tree
    // in reverse order, such that dirents for leaf directories
    // are created before parents. Then, the other dirents can set their size
    // by tabulation.

    let mut dirtree_count_cb = |entry_count: usize| {
        progress_callback(ProgressInfo::DiscoveredDirectory(entry_count));
    };
    let dirtree = fs::dir_tree(fs, &mut dirtree_count_cb)
        .await
        .map_err(WriteError::FilesystemHierarchyError)?;
    let (dirent_tables, dirent_count) =
        create_dirent_tables::<AvlDirectoryEntryTableBuilder>(&dirtree)?;
    // TODO: Maybe dirent_tables can be a Vec<(Path, Dirtab)>?
    // The order should already be guaranteed by dir_tree's invariants

    progress_callback(ProgressInfo::FileCount(dirent_count));
    progress_callback(ProgressInfo::DirCount(dirtree.len()));

    // Now we can forward iterate through the dirtabs and allocate on-disk regions
    let mut dir_sectors: BTreeMap<PathVec, u64> = BTreeMap::new();
    let mut sector_allocator = sector::SectorAllocator::default();

    let root_dirtab = dirent_tables
        .first_key_value()
        .expect("should always have one dirent at minimum (root)");
    let root_dirtab_size = root_dirtab.1.dirtab_size();
    let root_sector = sector_allocator.allocate_contiguous(root_dirtab_size as u64);
    dir_sectors.insert((*root_dirtab.0).clone(), root_sector as u64);

    for (path, dirtab) in dirent_tables.into_iter() {
        let dirtab_sector = dir_sectors
            .get(path)
            .expect("subdir sector allocation should have been previously computed");
        let dirtab = dirtab.disk_repr(&mut sector_allocator)?;
        progress_callback(ProgressInfo::DirAdded(
            fs.path_to_string(path),
            *dirtab_sector,
        ));

        image
            .write(
                dirtab_sector * layout::SECTOR_SIZE as u64,
                &dirtab.entry_table,
            )
            .await
            .map_err(WriteError::BlockDeviceError)?;

        for entry in dirtab.file_listing {
            // TODO: Replace with PathRef::join
            let file_path = PathVec::from_base(path, entry.name.as_str());
            progress_callback(ProgressInfo::FileAdded(
                fs.path_to_string(&file_path),
                entry.sector,
            ));

            if entry.is_dir {
                dir_sectors.insert(file_path, entry.sector);
            } else {
                fs.copy_file_in(
                    &file_path,
                    image,
                    0,
                    entry.sector * layout::SECTOR_SIZE as u64,
                    entry.size,
                )
                .await
                .map_err(WriteError::FilesystemCopierError)?;
            }
        }
    }

    progress_callback(ProgressInfo::FinishedCopyingImageData);

    write_volume_descriptor::<BDW, FS>(image, filetime, root_dirtab_size, root_sector).await?;
    apply_image_alignment_padding::<BDW, FS>(image).await?;

    progress_callback(ProgressInfo::FinishedPacking);
    Ok(())
}

#[cfg(test)]
mod test {
    use alloc::string::ToString;

    use alloc::vec;
    use alloc::vec::Vec;

    use crate::blockdev::NullBlockDevice;
    use crate::layout;
    use crate::write::dirtab::{
        AvlDirectoryEntryTableBuilder, DirectoryEntryTableBuilder, DirectoryEntryTableWriter,
    };
    use crate::write::fs::{
        FileEntry, MemoryFilesystem, PathVec, SectorLinearBlockDevice, SectorLinearBlockFilesystem,
        SectorLinearBlockSectorContents,
    };

    use super::fs::DirectoryTreeEntry;
    use super::{create_dirent_tables, create_xdvdfs_image, ProgressInfo};

    #[derive(Default)]
    struct MockDirtabBuilder(Vec<(alloc::string::String, u32, bool)>);

    impl DirectoryEntryTableBuilder for MockDirtabBuilder {
        type DirtabWriter = Self;

        fn add_file(
            &mut self,
            name: &str,
            size: u32,
        ) -> Result<(), crate::write::FileStructureError> {
            self.0.push((name.to_string(), size, true));
            Ok(())
        }

        fn add_dir(
            &mut self,
            name: &str,
            size: u32,
        ) -> Result<(), crate::write::FileStructureError> {
            self.0.push((name.to_string(), size, false));
            Ok(())
        }

        fn build(self) -> Result<Self::DirtabWriter, crate::write::FileStructureError> {
            Ok(self)
        }
    }

    impl DirectoryEntryTableWriter for MockDirtabBuilder {
        fn dirtab_size(&self) -> u32 {
            self.0.len() as u32
        }
    }

    #[test]
    fn test_create_dirent_tables_empty_root() {
        let dirtree = &[DirectoryTreeEntry {
            dir: PathVec::default(),
            listing: Vec::new(),
        }];

        let (dirtabs, count) = create_dirent_tables::<AvlDirectoryEntryTableBuilder>(dirtree)
            .expect("Dirtab should be valid");
        assert_eq!(count, 0);
        assert_eq!(dirtabs.len(), 1);
    }

    #[test]
    fn test_create_dirent_tables_root_file() {
        let dirtree = &[DirectoryTreeEntry {
            dir: PathVec::default(),
            listing: vec![FileEntry {
                name: "abc".to_string(),
                file_type: crate::write::fs::FileType::File,
                len: 10,
            }],
        }];

        let (dirtabs, count) = create_dirent_tables::<AvlDirectoryEntryTableBuilder>(dirtree)
            .expect("Dirtab should be valid");
        assert_eq!(count, 1);
        assert_eq!(dirtabs.len(), 1);
    }

    #[test]
    fn test_create_dirent_tables_root_directory() {
        let dirtree = &[
            DirectoryTreeEntry {
                dir: PathVec::default(),
                listing: vec![FileEntry {
                    name: "abc".to_string(),
                    file_type: crate::write::fs::FileType::Directory,
                    len: 0,
                }],
            },
            DirectoryTreeEntry {
                dir: "/abc".into(),
                listing: Vec::new(),
            },
        ];

        let (dirtabs, count) = create_dirent_tables::<AvlDirectoryEntryTableBuilder>(dirtree)
            .expect("Dirtab should be valid");
        assert_eq!(count, 1);
        assert_eq!(dirtabs.len(), 2);
    }

    #[test]
    fn test_create_dirent_tables_nested_dirs() {
        let dirtree = &[
            DirectoryTreeEntry {
                dir: PathVec::default(),
                listing: vec![FileEntry {
                    name: "abc".to_string(),
                    file_type: crate::write::fs::FileType::Directory,
                    len: 0,
                }],
            },
            DirectoryTreeEntry {
                dir: "/abc".into(),
                listing: vec![FileEntry {
                    name: "def".to_string(),
                    file_type: crate::write::fs::FileType::File,
                    len: 5,
                }],
            },
        ];

        let (dirtabs, count) = create_dirent_tables::<AvlDirectoryEntryTableBuilder>(dirtree)
            .expect("Dirtab should be valid");
        assert_eq!(count, 2);
        assert_eq!(dirtabs.len(), 2);
    }

    #[test]
    fn test_create_dirent_tables_entries_added() {
        let dirtree = &[
            DirectoryTreeEntry {
                dir: PathVec::default(),
                listing: vec![FileEntry {
                    name: "abc".to_string(),
                    file_type: crate::write::fs::FileType::Directory,
                    len: 0,
                }],
            },
            DirectoryTreeEntry {
                dir: "/abc".into(),
                listing: vec![FileEntry {
                    name: "def".to_string(),
                    file_type: crate::write::fs::FileType::File,
                    len: 5,
                }],
            },
        ];

        let (dirtabs, count) =
            create_dirent_tables::<MockDirtabBuilder>(dirtree).expect("Dirtab should be valid");
        assert_eq!(count, 2);
        assert_eq!(dirtabs.len(), 2);

        let root = dirtabs.first_key_value().unwrap();
        assert_eq!(*root.0, &PathVec::default());
        assert_eq!(root.1 .0, &[("abc".to_string(), 1, false),]);

        let abc = dirtabs.last_key_value().unwrap();
        assert_eq!(*abc.0, &PathVec::from("abc"));
        assert_eq!(abc.1 .0, &[("def".to_string(), 5, true),]);
    }

    #[test]
    fn test_create_xdvdfs_image_progress_callback() {
        let mut memfs = MemoryFilesystem::default();
        memfs.mkdir("/a");
        memfs.touch("/a/b");
        memfs.mkdir("/b");

        let mut nulldev = NullBlockDevice::default();

        let mut progress_list = Vec::new();

        let res =
            futures::executor::block_on(create_xdvdfs_image(&mut memfs, &mut nulldev, |pi| {
                progress_list.push(pi)
            }));
        assert!(res.is_ok());

        assert_eq!(
            progress_list,
            &[
                ProgressInfo::DiscoveredDirectory(2),
                ProgressInfo::DiscoveredDirectory(0),
                ProgressInfo::DiscoveredDirectory(1),
                ProgressInfo::FileCount(3),
                ProgressInfo::DirCount(3),
                ProgressInfo::DirAdded("/".to_string(), 33),
                ProgressInfo::FileAdded("/a".to_string(), 34),
                ProgressInfo::FileAdded("/b".to_string(), 35),
                ProgressInfo::DirAdded("/a".to_string(), 34),
                ProgressInfo::FileAdded("/a/b".to_string(), 36),
                ProgressInfo::DirAdded("/b".to_string(), 35),
                ProgressInfo::FinishedCopyingImageData,
                ProgressInfo::FinishedPacking,
            ]
        );
    }

    #[test]
    fn test_create_xdvdfs_image_32_sector_padding() {
        let mut memfs = MemoryFilesystem::default();
        let mut nulldev = NullBlockDevice::default();

        let res =
            futures::executor::block_on(create_xdvdfs_image(&mut memfs, &mut nulldev, |_| {}));
        assert!(res.is_ok());

        // Volume info at sector 32
        // Root dirent (empty) at sector 33
        // Padding up to 64
        assert_eq!(nulldev.len_blocking(), 64 * layout::SECTOR_SIZE as u64);
    }

    #[test]
    fn test_create_xdvdfs_image_32_sector_padding_not_applied() {
        let mut memfs = MemoryFilesystem::default();
        for i in 0..30 {
            // Volume info at sector 32
            // Root at sector 33 and 34
            // Need 30 more sectors to reach 64
            // (next multiple of 32-sector alignment)
            memfs.mkdir(std::format!("/{i}").as_str());
        }

        let mut slbd = SectorLinearBlockDevice::default();
        let mut slbfs = SectorLinearBlockFilesystem::new(memfs);

        let res = futures::executor::block_on(create_xdvdfs_image(&mut slbfs, &mut slbd, |_| {}));
        assert!(res.is_ok());

        assert_eq!(slbd.num_sectors(), 64);

        // An empty sector is 0xff filled,
        // whereas padding is 0x00 filled
        let empty_dirent_sector = alloc::vec![0xff; 2048];
        assert_eq!(
            slbd.get(63),
            SectorLinearBlockSectorContents::RawData(&empty_dirent_sector)
        );
    }

    #[test]
    fn test_create_xdvdfs_image_directories_and_files_copied_in() {
        let mut memfs = MemoryFilesystem::default();
        memfs.mkdir("/a");
        memfs.touch("/a/b");
        memfs.mkdir("/b");

        let mut slbd = SectorLinearBlockDevice::default();
        let mut slbfs = SectorLinearBlockFilesystem::new(memfs);

        let mut progress_list = Vec::new();

        let res = futures::executor::block_on(create_xdvdfs_image(&mut slbfs, &mut slbd, |pi| {
            progress_list.push(pi)
        }));
        assert!(res.is_ok());

        // Check directory tables are written
        // Volume descriptor
        assert!(matches!(
            slbd.get(32),
            SectorLinearBlockSectorContents::RawData(_)
        ));
        // "/"
        assert!(matches!(
            slbd.get(33),
            SectorLinearBlockSectorContents::RawData(_)
        ));
        // "/a"
        assert!(matches!(
            slbd.get(34),
            SectorLinearBlockSectorContents::RawData(_)
        ));
        // "/b"
        assert!(matches!(
            slbd.get(35),
            SectorLinearBlockSectorContents::RawData(_)
        ));
        // "/a/b"
        assert_eq!(
            slbd.get(36),
            SectorLinearBlockSectorContents::File("/a/b".into())
        );
    }
}
