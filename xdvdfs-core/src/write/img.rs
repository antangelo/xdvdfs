use std::path::{Path, PathBuf};

use alloc::collections::BTreeMap;

use crate::blockdev::BlockDeviceWrite;
use crate::write::{dirtab, fs, sector};
use crate::{layout, util};

use alloc::vec;
use alloc::vec::Vec;

/// Returns a recursive listing of paths in reverse order
/// e.g. for a path hierarchy like this:
/// /
/// -- /a
/// -- -- /a/b
/// -- /b
/// It might return the list: ["/b", "/a/b", "/a", "/"]
fn dir_tree<H: BlockDeviceWrite<E>, E>(
    root: &Path,
    fs: &impl fs::Filesystem<H, E>,
) -> Result<Vec<fs::DirectoryTreeEntry>, E> {
    let mut dirs = vec![PathBuf::from(root)];

    let mut out = Vec::new();

    while let Some(dir) = dirs.pop() {
        let listing = fs.read_dir(&dir)?;

        for entry in listing.iter() {
            if let fs::FileType::Directory = entry.file_type {
                dirs.push(entry.path.clone());
            }
        }

        out.push(fs::DirectoryTreeEntry { dir, listing });
    }

    out.reverse();
    Ok(out)
}

pub fn create_xdvdfs_image<H: BlockDeviceWrite<E>, E>(
    source_dir: &Path,
    fs: &impl fs::Filesystem<H, E>,
    image: &mut H,
) -> Result<(), util::Error<E>> {
    // We need to compute the size of all dirent tables before
    // writing the image. As such, we iterate over a directory tree
    // in reverse order, such that dirents for leaf directories
    // are created before parents. Then, the other dirents can set their size
    // by tabulation.

    let dirtree = dir_tree(source_dir, fs)?;
    let mut dirent_tables: BTreeMap<&PathBuf, dirtab::DirectoryEntryTableWriter> = BTreeMap::new();

    for entry in dirtree.iter() {
        let path = &entry.dir;
        let dir_entries = &entry.listing;

        let mut dirtab = dirtab::DirectoryEntryTableWriter::default();

        for entry in dir_entries {
            let file_name = entry.path.file_name().unwrap();
            let file_name = file_name.to_str().unwrap();

            match entry.file_type {
                fs::FileType::Directory => {
                    let dir_size = dirent_tables.get(&entry.path).unwrap().dirtab_size();
                    let dir_size = dir_size.try_into().unwrap();
                    dirtab.add_dir(file_name, dir_size)?;
                }
                fs::FileType::File => {
                    let file_size = entry.len.try_into().unwrap();
                    dirtab.add_file(file_name, file_size)?;
                }
            }
        }

        dirent_tables.insert(path, dirtab);
    }

    // Now we can forward iterate through the dirtabs and allocate on-disk regions
    let mut dir_sectors: BTreeMap<PathBuf, usize> = BTreeMap::new();
    let mut sector_allocator = sector::SectorAllocator::default();

    let root_dirtab = dirent_tables.first_key_value().unwrap();
    let root_sector = sector_allocator.allocate_contiguous(root_dirtab.1.dirtab_size());
    let root_table =
        layout::DirectoryEntryTable::new(root_dirtab.1.dirtab_size() as u32, root_sector as u32);
    dir_sectors.insert(root_dirtab.0.to_path_buf(), root_sector);

    for (path, dirtab) in dirent_tables.into_iter() {
        let dirtab_sector = dir_sectors.get(path).unwrap();
        std::println!("Adding dir: {:?} at sector {}", path, dirtab_sector);
        let dirtab = dirtab.disk_repr(&mut sector_allocator)?;

        BlockDeviceWrite::write(
            image,
            dirtab_sector * layout::SECTOR_SIZE,
            &dirtab.entry_table,
        )?;

        let dirtab_len = dirtab.entry_table.len();
        let padding_len = 2048 - dirtab_len % 2048;
        if padding_len < 2048 {
            let padding = vec![0xff; padding_len];
            BlockDeviceWrite::write(
                image,
                dirtab_sector * layout::SECTOR_SIZE + dirtab_len,
                &padding,
            )?;
        }

        for entry in dirtab.file_listing {
            let file_path = path.join(&entry.name);
            std::println!("Adding file: {:?} at sector {}", file_path, entry.sector);

            if entry.is_dir {
                dir_sectors.insert(file_path.clone(), entry.sector);
            } else {
                let file_len =
                    fs.copy_file_in(&file_path, image, entry.sector * layout::SECTOR_SIZE)?
                        as usize;
                let padding_len = 2048 - file_len % 2048;
                if padding_len < 2048 {
                    let padding = vec![0xff; padding_len];
                    BlockDeviceWrite::write(
                        image,
                        entry.sector * layout::SECTOR_SIZE + file_len,
                        &padding,
                    )?;
                }
            }
        }
    }

    // Write volume info to sector 32
    // FIXME: Set timestamp
    let volume_info = layout::VolumeDescriptor::new(root_table);
    let volume_info = volume_info.serialize()?;

    BlockDeviceWrite::write(image, 32 * layout::SECTOR_SIZE, &volume_info)?;

    let len = BlockDeviceWrite::len(image)? as usize;
    if len % (32 * 2048) > 0 {
        let padding = (32 * 2048) - len % (32 * 2048);
        let padding = vec![0xff; padding];
        BlockDeviceWrite::write(image, len, &padding)?;
    }

    Ok(())
}
