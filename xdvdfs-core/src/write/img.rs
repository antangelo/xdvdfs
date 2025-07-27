use crate::blockdev::BlockDeviceWrite;
use crate::layout;
use crate::write::{fs, sector};

use alloc::vec;
use alloc::vec::Vec;

use super::dirtab::{
    AvlDirectoryEntryTableBuilder, DirectoryEntryTableBuilder, DirectoryEntryTableWriter,
};
use super::fs::{
    DirectoryTreeEntry, FilesystemCopier, FilesystemHierarchy, PathCow, PathRef, PathVec,
};
use super::{FileStructureError, WriteError};

use maybe_async::maybe_async;

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProgressInfo<'a> {
    DiscoveredDirectory(usize),
    FileCount(usize),
    DirCount(usize),
    DirAdded(PathCow<'a>, u64),
    FileAdded(PathCow<'a>, u64),
    FinishedCopyingImageData,
    FinishedPacking,
}

#[non_exhaustive]
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OwnedProgressInfo {
    DiscoveredDirectory(usize),
    FileCount(usize),
    DirCount(usize),
    DirAdded(PathVec, u64),
    FileAdded(PathVec, u64),
    FinishedCopyingImageData,
    FinishedPacking,
}

impl ProgressInfo<'_> {
    pub fn to_owned(self) -> OwnedProgressInfo {
        match self {
            Self::DiscoveredDirectory(len) => OwnedProgressInfo::DiscoveredDirectory(len),
            Self::FileCount(count) => OwnedProgressInfo::FileCount(count),
            Self::DirCount(count) => OwnedProgressInfo::DirCount(count),
            Self::DirAdded(path, size) => OwnedProgressInfo::DirAdded(path.to_owned(), size),
            Self::FileAdded(path, size) => OwnedProgressInfo::FileAdded(path.to_owned(), size),
            Self::FinishedCopyingImageData => OwnedProgressInfo::FinishedCopyingImageData,
            Self::FinishedPacking => OwnedProgressInfo::FinishedPacking,
        }
    }
}

type DirentTableVec<'a, DTW> = Vec<(PathRef<'a>, DTW)>;

fn create_dirent_tables<'a, DTB: DirectoryEntryTableBuilder<'a>>(
    dirtree: &'a [DirectoryTreeEntry],
) -> Result<(DirentTableVec<'a, DTB::DirtabWriter>, usize), FileStructureError> {
    let mut dirent_tables: DirentTableVec<'_, DTB::DirtabWriter> =
        Vec::with_capacity(dirtree.len());
    let mut dtab_size_map: Vec<u32> = vec![0u32; dirtree.len()];
    let mut count = 0;

    for (dir_idx, entry) in dirtree.iter().enumerate().rev() {
        let path = &entry.dir;
        let dir_entries = &entry.listing;

        let mut dirtab = DTB::default();
        dirtab.reserve(dir_entries.len());

        for (entry, dir_index) in dir_entries {
            let file_name = entry.name.as_str();
            count += 1;

            match entry.file_type {
                fs::FileType::Directory => {
                    debug_assert_eq!(
                        dirtree[*dir_index].dir.as_path_ref(),
                        path.as_path_ref().join(file_name),
                    );

                    let dir_size = dtab_size_map[*dir_index];
                    debug_assert_ne!(dir_size, 0);
                    dirtab.add_dir(file_name, dir_size, *dir_index)?;
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

        // Store index of directory in dirent_tables in the dtab_size_map,
        // then pass it through the dirtab writer so it can be used as to look-up
        // sectors in the forward pass.
        let dtw = dirtab.build()?;
        let path = PathRef::from(path);

        dtab_size_map[dir_idx] = dtw.dirtab_size();
        dirent_tables.push((path, dtw));
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
    root_dirtab_size: u32,
    root_dirtab_sector: u32,
) -> Result<(), GenericWriteError<BDW, FS>> {
    // FIXME: Set timestamp
    let root_table = layout::DirectoryEntryTable::new(root_dirtab_size, root_dirtab_sector);
    let volume_info = layout::VolumeDescriptor::new(root_table);
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

    progress_callback(ProgressInfo::FileCount(dirent_count));
    progress_callback(ProgressInfo::DirCount(dirtree.len()));

    // Now we can forward iterate through the dirtabs and allocate on-disk regions
    let mut sector_allocator = sector::SectorAllocator::default();
    let mut dir_sectors: Vec<u64> = vec![0u64; dirent_tables.len()];

    let root_dirtab = dirent_tables
        .last()
        .expect("should always have one dirent at minimum (root)");
    let root_dirtab_size = root_dirtab.1.dirtab_size();
    let root_sector = sector_allocator.allocate_contiguous(root_dirtab_size as u64);
    dir_sectors[0] = root_sector as u64;

    for (dir_idx, (path, dirtab)) in dirent_tables.into_iter().rev().enumerate() {
        let dirtab_sector = dir_sectors[dir_idx];

        let dirtab = dirtab.disk_repr(&mut sector_allocator)?;
        progress_callback(ProgressInfo::DirAdded(path.into(), dirtab_sector));

        image
            .write(
                dirtab_sector * layout::SECTOR_SIZE as u64,
                &dirtab.entry_table,
            )
            .await
            .map_err(WriteError::BlockDeviceError)?;

        for entry in dirtab.file_listing {
            let file_path = PathRef::Join(&path, entry.name);
            progress_callback(ProgressInfo::FileAdded(file_path.into(), entry.sector));

            if entry.is_dir {
                debug_assert_eq!(dirtree[entry.idx].dir.as_path_ref(), file_path);
                dir_sectors[entry.idx] = entry.sector;
            } else {
                fs.copy_file_in(
                    file_path,
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

    write_volume_descriptor::<BDW, FS>(image, root_dirtab_size, root_sector).await?;
    apply_image_alignment_padding::<BDW, FS>(image).await?;

    progress_callback(ProgressInfo::FinishedPacking);
    Ok(())
}

#[cfg(test)]
mod test {
    use alloc::borrow::Cow;
    use alloc::string::ToString;

    use alloc::vec;
    use alloc::vec::Vec;

    use crate::blockdev::NullBlockDevice;
    use crate::layout;
    use crate::write::dirtab::{
        AvlDirectoryEntryTableBuilder, DirectoryEntryTableBuilder, DirectoryEntryTableWriter,
    };
    use crate::write::fs::{
        FileEntry, MemoryFilesystem, PathRef, PathVec, SectorLinearBlockDevice,
        SectorLinearBlockFilesystem, SectorLinearBlockSectorContents,
    };
    use crate::write::img::OwnedProgressInfo;

    use super::fs::DirectoryTreeEntry;
    use super::{create_dirent_tables, create_xdvdfs_image, ProgressInfo};

    #[derive(Default)]
    struct MockDirtabBuilder(Vec<(alloc::string::String, u32, bool, usize)>);

    impl<'alloc> DirectoryEntryTableBuilder<'alloc> for MockDirtabBuilder {
        type DirtabWriter = Self;

        fn add_file<N: Into<Cow<'alloc, str>>>(
            &mut self,
            name: N,
            size: u32,
        ) -> Result<(), crate::write::FileStructureError> {
            self.0.push((name.into().to_string(), size, true, 0));
            Ok(())
        }

        fn add_dir<N: Into<Cow<'alloc, str>>>(
            &mut self,
            name: N,
            size: u32,
            idx: usize,
        ) -> Result<(), crate::write::FileStructureError> {
            self.0.push((name.into().to_string(), size, false, idx));
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
            dir: "".into(),
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
            dir: "".into(),
            listing: vec![(
                FileEntry {
                    name: "abc".to_string(),
                    file_type: crate::write::fs::FileType::File,
                    len: 10,
                },
                0,
            )],
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
                dir: "".into(),
                listing: vec![(
                    FileEntry {
                        name: "abc".to_string(),
                        file_type: crate::write::fs::FileType::Directory,
                        len: 0,
                    },
                    1,
                )],
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
                dir: "".into(),
                listing: vec![(
                    FileEntry {
                        name: "abc".to_string(),
                        file_type: crate::write::fs::FileType::Directory,
                        len: 0,
                    },
                    1,
                )],
            },
            DirectoryTreeEntry {
                dir: "/abc".into(),
                listing: vec![(
                    FileEntry {
                        name: "def".to_string(),
                        file_type: crate::write::fs::FileType::File,
                        len: 5,
                    },
                    0,
                )],
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
                dir: "".into(),
                listing: vec![(
                    FileEntry {
                        name: "abc".to_string(),
                        file_type: crate::write::fs::FileType::Directory,
                        len: 0,
                    },
                    1,
                )],
            },
            DirectoryTreeEntry {
                dir: "/abc".into(),
                listing: vec![(
                    FileEntry {
                        name: "def".to_string(),
                        file_type: crate::write::fs::FileType::File,
                        len: 5,
                    },
                    0,
                )],
            },
        ];

        let (dirtabs, count) =
            create_dirent_tables::<MockDirtabBuilder>(dirtree).expect("Dirtab should be valid");
        assert_eq!(count, 2);
        assert_eq!(dirtabs.len(), 2);

        let root = dirtabs.last().unwrap();
        assert_eq!(root.0, PathVec::default());
        assert_eq!(root.1 .0, &[("abc".to_string(), 1, false, 1)]);

        let abc = dirtabs.first().unwrap();
        assert_eq!(abc.0, PathVec::from("abc"));
        assert_eq!(abc.1 .0, &[("def".to_string(), 5, true, 0)]);
    }

    #[test]
    fn test_create_xdvdfs_image_progress_callback() {
        let mut memfs = MemoryFilesystem::default();
        memfs.mkdir("/a");
        memfs.touch("/a/b");
        memfs.mkdir("/b");

        let mut nulldev = NullBlockDevice::default();

        let mut progress_list = Vec::new();

        let res = futures::executor::block_on(create_xdvdfs_image(
            &mut memfs,
            &mut nulldev,
            |pi: ProgressInfo<'_>| progress_list.push(pi.to_owned()),
        ));
        assert!(res.is_ok());

        assert_eq!(
            progress_list,
            &[
                OwnedProgressInfo::DiscoveredDirectory(2),
                OwnedProgressInfo::DiscoveredDirectory(1),
                OwnedProgressInfo::DiscoveredDirectory(0),
                OwnedProgressInfo::FileCount(3),
                OwnedProgressInfo::DirCount(3),
                OwnedProgressInfo::DirAdded(PathRef::from("/").into(), 33),
                OwnedProgressInfo::FileAdded(PathRef::from("/a").into(), 34),
                OwnedProgressInfo::FileAdded(PathRef::from("/b").into(), 35),
                OwnedProgressInfo::DirAdded(PathRef::from("/a").into(), 34),
                OwnedProgressInfo::FileAdded(PathRef::from("/a/b").into(), 36),
                OwnedProgressInfo::DirAdded(PathRef::from("/b").into(), 35),
                OwnedProgressInfo::FinishedCopyingImageData,
                OwnedProgressInfo::FinishedPacking,
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

        let res = futures::executor::block_on(create_xdvdfs_image(&mut slbfs, &mut slbd, |_| {}));
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
