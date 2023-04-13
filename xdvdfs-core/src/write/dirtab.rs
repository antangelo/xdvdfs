use crate::layout::{
    DirectoryEntryData, DirectoryEntryDiskData, DirectoryEntryTable, DirentAttributes, DiskRegion,
};
use crate::util;
use crate::write::avl;

use alloc::{boxed::Box, string::String, vec::Vec};

use super::sector::SectorAllocator;

/// Writer for directory entry tables
pub struct DirectoryEntryTableWriter {
    table: avl::AvlTree<DirectoryEntryData>,

    size: usize,
}

impl Default for DirectoryEntryTableWriter {
    fn default() -> Self {
        Self {
            table: avl::AvlTree::default(),
            size: 0,
        }
    }
}

impl DirectoryEntryTableWriter {
    fn add_node<E>(
        &mut self,
        name: &str,
        size: u32,
        attributes: DirentAttributes,
    ) -> Result<(), util::Error<E>> {
        let name = arrayvec::ArrayString::from(name).map_err(|_| util::Error::NameTooLong)?;
        let filename_length = name
                    .len()
                    .try_into()
                    .map_err(|_| util::Error::NameTooLong)?;

        self.table.insert(DirectoryEntryData {
            node: DirectoryEntryDiskData {
                data: DiskRegion { sector: 0, size },
                attributes,
                filename_length,
            },
            name,
        });

        let size = 0xe + name.len();
        self.size += size;

        if size % 4 != 0 {
            self.size += 4 - size % 4;
        }

        Ok(())
    }

    pub fn add_dir<E>(&mut self, name: &str, size: u32) -> Result<(), util::Error<E>> {
        let attributes = DirentAttributes(0).with_directory(true);
        self.add_node(name, size, attributes)
    }

    pub fn add_file<E>(&mut self, name: &str, size: u32) -> Result<(), util::Error<E>> {
        let attributes = DirentAttributes(0).with_archive(true);
        self.add_node(name, size, attributes)
    }

    /// Returns the size of the directory entry table, in bytes.
    pub fn dirtab_size(&self) -> usize {
        self.size
    }

    /// Allocates and writes the directory entry table to disk. This happens
    /// in several steps.
    ///
    /// 1. Allocate sectors for each file
    /// 2. Build a map between file path/sectors
    /// 3. Write out the directory table at the provided disk region
    /// 4. Update directory entries to set allocated sector offset
    ///
    /// Returns a byte slice representing the on-disk directory entry table,
    /// the sector that this dirent will occupy,
    /// and a mapping of files to allocated sectors.
    pub fn to_disk_repr<E>(
        self,
        table: DirectoryEntryTable,
        allocator: &mut SectorAllocator,
    ) -> Result<(Box<[u8]>, usize, Vec<(String, usize)>), util::Error<E>> {
        let dirent_sector = allocator.allocate_contiguous(self.dirtab_size());
        let dirent_bytes: Vec<u8> = Vec::new();

        todo!();
    }
}
