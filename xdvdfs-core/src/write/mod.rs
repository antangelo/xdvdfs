use std::fs::{DirEntry, Metadata};
use std::path::{Path, PathBuf};

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use crate::util;

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
    let image = std::fs::File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(output)?;

    let sector_allocator = sector::SectorAllocator::default();

    // We need to compute the size of all dirent tables before
    // writing the image. As such, we iterate over a directory tree
    // in reverse order, such that dirents for leaf directories
    // are created before parents. Then, the other dirents can set their size
    // by tabulation.

    let dirtree = dir_tree(source_dir)?;
    let mut dirent_tables: BTreeMap<&PathBuf, dirtab::DirectoryEntryTableWriter> = BTreeMap::new();

    for (path, dir_entries) in dirtree.iter() {
        let mut dirtab = dirtab::DirectoryEntryTableWriter::default();
        //std::println!("path: {:?}", path);

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
    for (path, dirtab) in dirent_tables.iter() {
        std::println!("dtpath: {:?}", path);
    }

    Ok(())
}
