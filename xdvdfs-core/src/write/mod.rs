use std::fs::{DirEntry, Metadata};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use alloc::borrow::ToOwned;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use bincode::Options;

use crate::{util, layout};

pub mod avl;
mod dirtab;
mod sector;

/// Returns a recursive listing of paths in reverse order
/// e.g. for a path hierarchy like this:
/// /
/// -- /a
/// -- -- /a/b
/// -- /b
/// It might return the list: ["/b", "/a/b", "/a", "/"]
fn dir_tree(root: &Path) -> std::io::Result<Vec<(PathBuf, Vec<(DirEntry, Metadata)>)>> {
    let mut dirs = vec![PathBuf::from(root)];

    let mut out = Vec::new();

    while let Some(dir) = dirs.pop() {
        let listing = std::fs::read_dir(&dir)?;
        let listing: std::io::Result<Vec<DirEntry>> = listing.collect();
        let listing: std::io::Result<Vec<(DirEntry, Metadata)>> = listing?
            .into_iter()
            .map(|de| de.metadata().map(|md| (de, md)))
            .collect();
        let listing = listing?;

        for (subdir, metadata) in listing.iter() {
            if metadata.is_dir() {
                dirs.push(subdir.path());
            }
        }

        out.push((dir, listing));
    }

    out.reverse();
    Ok(out)
}

pub fn create_xdvdfs_image(
    source_dir: &Path,
    output: &Path,
) -> Result<(), util::Error<std::io::Error>> {
    let mut image = std::fs::File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(output)?;

    // We need to compute the size of all dirent tables before
    // writing the image. As such, we iterate over a directory tree
    // in reverse order, such that dirents for leaf directories
    // are created before parents. Then, the other dirents can set their size
    // by tabulation.

    let dirtree = dir_tree(source_dir)?;
    let mut dirent_tables: BTreeMap<&PathBuf, dirtab::DirectoryEntryTableWriter> = BTreeMap::new();

    for (path, dir_entries) in dirtree.iter() {
        let mut dirtab = dirtab::DirectoryEntryTableWriter::default();

        for (entry, metadata) in dir_entries {
            let file_name = entry.file_name();
            let file_name = file_name.to_str().unwrap();
            if metadata.is_file() {
                let file_size = metadata.len().try_into().unwrap();
                dirtab.add_file(file_name, file_size)?;
            } else if metadata.is_dir() {
                let dir_size = dirent_tables.get(&entry.path()).unwrap().dirtab_size();
                let dir_size = dir_size.try_into().unwrap();
                dirtab.add_dir(file_name, dir_size)?;
            } else {
                std::println!("Skipping unknown file: {:?}", path);
            }
        }

        dirent_tables.insert(path, dirtab);
    }

    // Now we can forward iterate through the dirtabs and allocate on-disk regions
    let mut dir_sectors: BTreeMap<PathBuf, usize> = BTreeMap::new();
    let mut sector_allocator = sector::SectorAllocator::default();

    let root_dirtab = dirent_tables.first_key_value().unwrap();
    let root_sector = sector_allocator.allocate_contiguous(root_dirtab.1.dirtab_size());
    let root_table = layout::DirectoryEntryTable::new(root_dirtab.1.dirtab_size() as u32, root_sector as u32);
    dir_sectors.insert(root_dirtab.0.to_path_buf(), root_sector);

    for (path, dirtab) in dirent_tables.into_iter() {
        let dirtab_sector = dir_sectors.get(path).unwrap();
        std::println!("adding directory: {:?} at sector {}", path, dirtab_sector);
        let (dirtab, file_sector_map) = dirtab.to_disk_repr(&mut sector_allocator)?;

        image.seek(SeekFrom::Start((dirtab_sector * layout::SECTOR_SIZE) as u64))?;
        image.write_all(&dirtab)?;

        for (name, sector) in file_sector_map {
            let file_path = path.join(&name);
            //std::println!("Adding file: {:?}", file_path);
            
            let file_meta = std::fs::metadata(&file_path)?;

            if file_meta.is_dir() {
                dir_sectors.insert(file_path.clone(), sector);
                continue;
            }

            if !file_meta.is_file() {
                panic!("File with unsupported metadata found");
            }

            let mut file = std::fs::File::open(file_path)?;

            image.seek(SeekFrom::Start((sector * layout::SECTOR_SIZE) as u64))?;
            std::io::copy(&mut file, &mut image)?;
        }
    }

    // Write volume info to sector 32
    let volume_info = layout::VolumeDescriptor::new(root_table);
    let volume_info = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .serialize(&volume_info)
        .map_err(|e| util::Error::SerializationFailed(e))?;

    image.seek(SeekFrom::Start((32 * layout::SECTOR_SIZE) as u64))?;
    image.write_all(&volume_info)?;

    Ok(())
}
