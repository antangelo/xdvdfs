use alloc::collections::BTreeMap;
use alloc::string::String;

use crate::blockdev::BlockDeviceWrite;
use crate::layout;
use crate::write::{dirtab, fs, sector};

use alloc::vec;

use super::fs::{DirectoryTreeEntry, FilesystemCopier, FilesystemHierarchy, PathVec};
use super::{FileStructureError, WriteError};

use maybe_async::maybe_async;

#[non_exhaustive]
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum ProgressInfo {
    DiscoveredDirectory(usize),
    FileCount(usize),
    DirCount(usize),
    DirAdded(String, u64),
    FileAdded(String, u64),
    FinishedCopyingImageData,
    FinishedPacking,
}

type DirentTableMap<'a> = BTreeMap<&'a PathVec, dirtab::DirectoryEntryTableWriter>;

fn create_dirent_tables<'a>(
    dirtree: &'a [DirectoryTreeEntry],
) -> Result<(DirentTableMap<'a>, usize), FileStructureError> {
    let mut dirent_tables: DirentTableMap<'_> = BTreeMap::new();
    let mut count = 0;

    for entry in dirtree.iter().rev() {
        let path = &entry.dir;
        let dir_entries = &entry.listing;

        let mut dirtab = dirtab::DirectoryEntryTableBuilder::default();

        for entry in dir_entries {
            let file_name = entry.name.as_str();
            count += 1;

            match entry.file_type {
                fs::FileType::Directory => {
                    // TODO: Replace with PathRef::join
                    let entry_path = PathVec::from_base(path, file_name);
                    let dir_size = dirent_tables.get(&entry_path)
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
    BDW: BlockDeviceWrite,
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
    image.write(32 * layout::SECTOR_SIZE as u64, &volume_info)
        .await
        .map_err(WriteError::BlockDeviceError)?;
    Ok(())
}

#[maybe_async]
async fn apply_image_alignment_padding<
    BDW: BlockDeviceWrite,
    FS: FilesystemHierarchy + FilesystemCopier<BDW> + ?Sized,
>(
    image: &mut BDW,
) -> Result<(), GenericWriteError<BDW, FS>> {
    let len = image.len()
        .await
        .map_err(WriteError::BlockDeviceError)?;

    let aligned = len.next_multiple_of(32 * layout::SECTOR_SIZE as u64);
    let padding = aligned - len;
    let padding: usize = padding.try_into().expect("padding < 32 * SECTOR_SIZE");

    if padding > 0 {
        let padding = vec![0x00; padding as usize];
        image.write(len, &padding)
            .await
            .map_err(WriteError::BlockDeviceError)?;
    }

    Ok(())
}

#[maybe_async]
pub async fn create_xdvdfs_image<
    BDW: BlockDeviceWrite,
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
    let (dirent_tables, dirent_count) = create_dirent_tables(&dirtree)?;
    // TODO: Maybe dirent_tables can be a Vec<(Path, Dirtab)>?
    // The order should already be guaranteed by dir_tree's invariants

    progress_callback(ProgressInfo::FileCount(dirent_count));
    progress_callback(ProgressInfo::DirCount(dirtree.len()));

    // Now we can forward iterate through the dirtabs and allocate on-disk regions
    // TODO: See if this separate map can be eliminated
    // Maybe combine with dirent_tables using a refcell and iter()?
    // If dirent_tables is replaced with a Vec, consider a PPT
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

        image.write(
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

    write_volume_descriptor::<BDW, FS>(image, root_dirtab_size, root_sector).await?;
    apply_image_alignment_padding::<BDW, FS>(image).await?;

    progress_callback(ProgressInfo::FinishedPacking);
    Ok(())
}

#[cfg(test)]
mod test {
    use alloc::string::ToString;

    use alloc::vec;
    use alloc::vec::Vec;

    use crate::write::fs::{FileEntry, PathVec};

    use super::create_dirent_tables;
    use super::fs::DirectoryTreeEntry;

    #[test]
    fn test_create_dirent_tables_empty_root() {
        let dirtree = &[
            DirectoryTreeEntry {
                dir: PathVec::default(),
                listing: Vec::new(),
            }
        ];

        let (dirtabs, count) = create_dirent_tables(dirtree)
            .expect("Dirtab should be valid");
        assert_eq!(count, 0);
        assert_eq!(dirtabs.len(), 1);
    }

    #[test]
    fn test_create_dirent_tables_root_file() {
        let dirtree = &[
            DirectoryTreeEntry {
                dir: PathVec::default(),
                listing: vec![
                    FileEntry {
                        name: "abc".to_string(),
                        file_type: crate::write::fs::FileType::File,
                        len: 10,
                    }
                ],
            }
        ];

        let (dirtabs, count) = create_dirent_tables(dirtree)
            .expect("Dirtab should be valid");
        assert_eq!(count, 1);
        assert_eq!(dirtabs.len(), 1);
    }

    #[test]
    fn test_create_dirent_tables_root_directory() {
        let dirtree = &[
            DirectoryTreeEntry {
                dir: PathVec::default(),
                listing: vec![
                    FileEntry {
                        name: "abc".to_string(),
                        file_type: crate::write::fs::FileType::Directory,
                        len: 0,
                    }
                ],
            },
            DirectoryTreeEntry {
                dir: "/abc".into(),
                listing: Vec::new(),
            },
        ];

        let (dirtabs, count) = create_dirent_tables(dirtree)
            .expect("Dirtab should be valid");
        assert_eq!(count, 1);
        assert_eq!(dirtabs.len(), 2);
    }

    // TODO: Nested directory tests?
    // TODO: Verify dirents are being added to the builders?

}
