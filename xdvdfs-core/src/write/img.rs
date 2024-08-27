use alloc::collections::BTreeMap;
use alloc::string::String;

use crate::blockdev::BlockDeviceWrite;
use crate::write::{dirtab, fs, sector};
use crate::{layout, util};

use alloc::vec;
use alloc::vec::Vec;

use super::fs::{DirectoryTreeEntry, PathVec};

use maybe_async::maybe_async;

#[non_exhaustive]
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum ProgressInfo {
    DiscoveredDirectory(usize),
    FileCount(usize),
    DirAdded(String, u64),
    FileAdded(String, u64),
    FinishedPacking,
}

/// Returns a recursive listing of paths in reverse order
/// e.g. for a path hierarchy like this:
/// /
/// -- /a
/// -- -- /a/b
/// -- /b
/// It might return the list: ["/b", "/a/b", "/a", "/"]
#[maybe_async]
async fn dir_tree<H: BlockDeviceWrite<E>, E>(
    fs: &mut (impl fs::Filesystem<H, E> + ?Sized),
    progress_callback: &impl Fn(ProgressInfo),
) -> Result<Vec<fs::DirectoryTreeEntry>, E> {
    let mut dirs = vec![PathVec::default()];
    let mut out = Vec::new();

    while let Some(dir) = dirs.pop() {
        let listing = fs.read_dir(&dir).await?;
        progress_callback(ProgressInfo::DiscoveredDirectory(listing.len()));

        for entry in listing.iter() {
            if let fs::FileType::Directory = entry.file_type {
                dirs.push(PathVec::from_base(&dir, &entry.name));
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
) -> Result<BTreeMap<&'a PathVec, dirtab::DirectoryEntryTableWriter>, util::Error<E>> {
    let mut dirent_tables: BTreeMap<&PathVec, dirtab::DirectoryEntryTableWriter> = BTreeMap::new();
    let mut count = 0;

    for entry in dirtree.iter() {
        let path = &entry.dir;
        let dir_entries = &entry.listing;

        let mut dirtab = dirtab::DirectoryEntryTableWriter::default();

        for entry in dir_entries {
            let file_name = entry.name.as_str();
            count += 1;

            match entry.file_type {
                fs::FileType::Directory => {
                    // Unwrap note:
                    // 1. Algorithm runs in order such that previous dirents are already in the
                    //    map, failure in practice is an algorithmic bug and not a result of the
                    //    input.
                    // 2. Previous dirents should have their size computed. If they don't this is
                    //    an algorithmic bug.
                    let entry_path = PathVec::from_base(path, file_name);
                    let dir_size = dirent_tables.get(&entry_path).unwrap().dirtab_size();
                    dirtab.add_dir(file_name, dir_size)?;
                }
                fs::FileType::File => {
                    let file_size = entry
                        .len
                        .try_into()
                        .map_err(|_| util::Error::FileTooLarge)?;
                    dirtab.add_file(file_name, file_size)?;
                }
            }
        }

        dirtab.compute_size()?;
        dirent_tables.insert(path, dirtab);
    }

    progress_callback(ProgressInfo::FileCount(count));
    Ok(dirent_tables)
}

#[maybe_async]
pub async fn create_xdvdfs_image<H: BlockDeviceWrite<E>, E>(
    fs: &mut (impl fs::Filesystem<H, E> + ?Sized),
    image: &mut H,
    progress_callback: impl Fn(ProgressInfo),
) -> Result<(), util::Error<E>> {
    // We need to compute the size of all dirent tables before
    // writing the image. As such, we iterate over a directory tree
    // in reverse order, such that dirents for leaf directories
    // are created before parents. Then, the other dirents can set their size
    // by tabulation.

    let dirtree = dir_tree(fs, &progress_callback).await?;
    let dirent_tables = create_dirent_tables(&dirtree, &progress_callback)?;

    // Now we can forward iterate through the dirtabs and allocate on-disk regions
    let mut dir_sectors: BTreeMap<PathVec, u64> = BTreeMap::new();
    let mut sector_allocator = sector::SectorAllocator::default();

    let root_dirtab = dirent_tables
        .first_key_value()
        .expect("should always have one dirent at minimum (root)");
    let root_dirtab_size = root_dirtab.1.dirtab_size();
    let root_sector = sector_allocator.allocate_contiguous(root_dirtab_size as u64);
    let root_table = layout::DirectoryEntryTable::new(root_dirtab_size, root_sector);
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

        BlockDeviceWrite::write(
            image,
            dirtab_sector * layout::SECTOR_SIZE as u64,
            &dirtab.entry_table,
        )
        .await?;

        for entry in dirtab.file_listing {
            let file_path = PathVec::from_base(path, entry.name.as_str());
            progress_callback(ProgressInfo::FileAdded(
                fs.path_to_string(&file_path),
                entry.sector,
            ));

            if entry.is_dir {
                dir_sectors.insert(file_path.clone(), entry.sector);
            } else {
                fs.copy_file_in(
                    &file_path,
                    image,
                    entry.sector * layout::SECTOR_SIZE as u64,
                    entry.size,
                )
                .await?;
            }
        }
    }

    // Write volume info to sector 32
    // FIXME: Set timestamp
    let volume_info = layout::VolumeDescriptor::new(root_table);
    let volume_info = volume_info.serialize()?;

    BlockDeviceWrite::write(image, 32 * layout::SECTOR_SIZE as u64, &volume_info).await?;

    let len = BlockDeviceWrite::len(image).await?;
    if len % (32 * 2048) > 0 {
        let padding = (32 * 2048) - len % (32 * 2048);
        let padding = vec![0x00; padding.try_into().unwrap()];
        BlockDeviceWrite::write(image, len, &padding).await?;
    }

    progress_callback(ProgressInfo::FinishedPacking);
    Ok(())
}
