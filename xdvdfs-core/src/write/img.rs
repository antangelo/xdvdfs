use std::path::{Path, PathBuf};

use alloc::collections::BTreeMap;

use crate::blockdev::BlockDeviceWrite;
use crate::util::ToUnexpectedError;
use crate::write::{dirtab, fs, sector};
use crate::{layout, util};

use alloc::vec;
use alloc::vec::Vec;

use super::fs::DirectoryTreeEntry;

/// Returns a recursive listing of paths in reverse order
/// e.g. for a path hierarchy like this:
/// /
/// -- /a
/// -- -- /a/b
/// -- /b
/// It might return the list: ["/b", "/a/b", "/a", "/"]
async fn dir_tree<H: BlockDeviceWrite<E>, E>(
    root: &Path,
    fs: &mut impl fs::Filesystem<H, E>,
) -> Result<Vec<fs::DirectoryTreeEntry>, E> {
    let mut dirs = vec![PathBuf::from(root)];

    let mut out = Vec::new();

    while let Some(dir) = dirs.pop() {
        let listing = fs.read_dir(&dir).await?;

        for entry in listing.iter() {
            if let fs::FileType::Directory = entry.file_type {
                dirs.push(entry.path.clone());
            }
        }

        out.push(fs::DirectoryTreeEntry { dir, listing });
    }

    // FIXME: Remove this and just use a reverse iterator
    out.reverse();
    Ok(out)
}

fn create_dirent_tables<'a, E>(
    dirtree: &'a [DirectoryTreeEntry],
    progress_callback: &impl Fn(ProgressInfo),
) -> Result<BTreeMap<&'a PathBuf, dirtab::DirectoryEntryTableWriter>, util::Error<E>> {
    let mut dirent_tables: BTreeMap<&PathBuf, dirtab::DirectoryEntryTableWriter> = BTreeMap::new();
    let mut count = 0;

    for entry in dirtree.iter() {
        let path = &entry.dir;
        let dir_entries = &entry.listing;

        let mut dirtab = dirtab::DirectoryEntryTableWriter::default();

        for entry in dir_entries {
            let file_name = entry
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or(util::Error::InvalidFileName)?;
            count += 1;

            match entry.file_type {
                fs::FileType::Directory => {
                    // Unwrap note:
                    // 1. Algorithm runs in order such that previous dirents are already in the
                    //    map, failure in practice is an algorithmic bug and not a result of the
                    //    input.
                    // 2. Previous dirents should have their size computed. If they don't this is
                    //    an algorithmic bug.
                    let dir_size = dirent_tables.get(&entry.path).unwrap().dirtab_size();
                    let dir_size = dir_size.try_into().or_unexpected()?;
                    dirtab.add_dir(file_name, dir_size)?;
                }
                fs::FileType::File => {
                    let file_size = entry.len.try_into().or_unexpected()?;
                    dirtab.add_file(file_name, file_size)?;
                }
            }
        }

        dirtab.compute_size();
        dirent_tables.insert(path, dirtab);
    }

    progress_callback(ProgressInfo::FileCount(count));
    Ok(dirent_tables)
}

#[non_exhaustive]
pub enum ProgressInfo {
    FileCount(usize),
    DirAdded(PathBuf, u64),
    FileAdded(PathBuf, u64),
    FinishedPacking,
}

pub async fn create_xdvdfs_image<H: BlockDeviceWrite<E>, E>(
    source_dir: &Path,
    fs: &mut impl fs::Filesystem<H, E>,
    image: &mut H,
    progress_callback: impl Fn(ProgressInfo),
) -> Result<(), util::Error<E>> {
    // We need to compute the size of all dirent tables before
    // writing the image. As such, we iterate over a directory tree
    // in reverse order, such that dirents for leaf directories
    // are created before parents. Then, the other dirents can set their size
    // by tabulation.

    let dirtree = dir_tree(source_dir, fs).await?;
    let dirent_tables = create_dirent_tables(&dirtree, &progress_callback)?;

    // Now we can forward iterate through the dirtabs and allocate on-disk regions
    let mut dir_sectors: BTreeMap<PathBuf, u64> = BTreeMap::new();
    let mut sector_allocator = sector::SectorAllocator::default();

    let root_dirtab = dirent_tables
        .first_key_value()
        .expect("should always have one dirent at minimum (root)");
    let root_dirtab_size = root_dirtab.1.dirtab_size();
    let root_sector = sector_allocator.allocate_contiguous(root_dirtab_size);
    let root_table = layout::DirectoryEntryTable::new(
        root_dirtab_size.try_into().or_unexpected()?,
        root_sector.try_into().or_unexpected()?,
    );
    dir_sectors.insert(root_dirtab.0.to_path_buf(), root_sector);

    for (path, dirtab) in dirent_tables.into_iter() {
        let dirtab_sector = dir_sectors
            .get(path)
            .expect("subdir sector allocation should have been previously computed");
        let dirtab = dirtab.disk_repr(&mut sector_allocator)?;
        progress_callback(ProgressInfo::DirAdded(path.to_path_buf(), *dirtab_sector));

        BlockDeviceWrite::write(
            image,
            dirtab_sector * layout::SECTOR_SIZE,
            &dirtab.entry_table,
        )
        .await?;

        for entry in dirtab.file_listing {
            let file_path = path.join(&entry.name);
            progress_callback(ProgressInfo::FileAdded(
                file_path.to_path_buf(),
                entry.sector,
            ));

            if entry.is_dir {
                dir_sectors.insert(file_path.clone(), entry.sector);
            } else {
                fs.copy_file_in(&file_path, image, entry.sector * layout::SECTOR_SIZE)
                    .await?;
            }
        }
    }

    // Write volume info to sector 32
    // FIXME: Set timestamp
    let volume_info = layout::VolumeDescriptor::new(root_table);
    let volume_info = volume_info.serialize()?;

    BlockDeviceWrite::write(image, 32 * layout::SECTOR_SIZE, &volume_info).await?;

    let len = BlockDeviceWrite::len(image).await?;
    if len % (32 * 2048) > 0 {
        let padding = (32 * 2048) - len % (32 * 2048);
        let padding = vec![0x00; padding.try_into().unwrap()];
        BlockDeviceWrite::write(image, len, &padding).await?;
    }

    progress_callback(ProgressInfo::FinishedPacking);
    Ok(())
}
