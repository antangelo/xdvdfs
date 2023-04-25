use crate::layout::{
    self, DirectoryEntryData, DirectoryEntryDiskData, DirectoryEntryDiskNode, DirentAttributes,
    DiskRegion,
};
use crate::util;
use crate::write::avl;

use alloc::string::ToString;
use alloc::{boxed::Box, string::String, vec::Vec};

use super::sector::{required_sectors, SectorAllocator};

/// Writer for directory entry tables
#[derive(Default)]
pub struct DirectoryEntryTableWriter {
    table: avl::AvlTree<DirectoryEntryData>,

    size: Option<u64>,
}

pub struct FileListingEntry {
    pub name: String,
    pub sector: u64,
    pub is_dir: bool,
}

pub struct DirectoryEntryTableDiskRepr {
    pub entry_table: Box<[u8]>,
    pub file_listing: Vec<FileListingEntry>,
}

fn sector_align(offset: u64, incr: u64) -> u64 {
    let used_sectors = required_sectors(offset);
    let needed_sectors = required_sectors(offset + incr);
    if offset % layout::SECTOR_SIZE > 0 && needed_sectors > used_sectors {
        layout::SECTOR_SIZE - offset % layout::SECTOR_SIZE
    } else {
        0
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

        let dirent = DirectoryEntryData {
            node: DirectoryEntryDiskData {
                data: DiskRegion { sector: 0, size },
                attributes,
                filename_length,
            },
            name,
        };

        self.size = None;
        self.table.insert(dirent);

        Ok(())
    }

    pub fn add_dir<E>(&mut self, name: &str, size: u32) -> Result<(), util::Error<E>> {
        let attributes = DirentAttributes(0).with_directory(true);

        let size = size + ((2048 - size % 2048) % 2048);
        self.add_node(name, size, attributes)
    }

    pub fn add_file<E>(&mut self, name: &str, size: u32) -> Result<(), util::Error<E>> {
        let attributes = DirentAttributes(0).with_archive(true);
        self.add_node(name, size, attributes)
    }

    pub fn compute_size(&mut self) {
        self.size = Some(
            self.table
                .preorder_iter()
                .map(|node| node.len_on_disk())
                .fold(0, |acc, disk_len: u64| {
                    acc + disk_len + sector_align(acc, disk_len)
                }),
        );
    }

    /// Returns the size of the directory entry table, in bytes.
    pub fn dirtab_size(&self) -> Option<u64> {
        // FS bug: zero sized dirents are listed as size 2048
        if self.table.backing_vec().is_empty() {
            Some(2048)
        } else {
            self.size
        }
    }

    /// Serializes directory entry table to a on-disk representation
    /// This function performs three steps:
    ///
    /// 1. Allocate sectors for each file
    /// 2. Build a map between file path/sectors
    /// 3. Update directory entries to set allocated sector offset
    ///
    /// Returns a byte slice representing the on-disk directory entry table,
    /// and a mapping of files to allocated sectors
    pub fn disk_repr<E>(
        mut self,
        allocator: &mut SectorAllocator,
    ) -> Result<DirectoryEntryTableDiskRepr, util::Error<E>> {
        self.table.reorder_backing_preorder();

        // Array of offsets for each entry in the table
        // The offset is a partial sum of lengths of the dirent on disk
        let mut offsets: Vec<u16> = self
            .table
            .backing_vec()
            .iter()
            .map(|node| node.data().len_on_disk().try_into().unwrap())
            .collect();
        if offsets.is_empty() {
            return Ok(DirectoryEntryTableDiskRepr {
                entry_table: alloc::vec![].into_boxed_slice(),
                file_listing: alloc::vec![],
            });
        }

        offsets.rotate_right(1);
        let final_dirent_size = offsets[0];
        offsets[0] = 0;
        for i in 1..offsets.len() {
            offsets[i] += offsets[i - 1];

            let next_size = if i + 1 == offsets.len() {
                final_dirent_size
            } else {
                offsets[i + 1]
            };
            let adj: u16 = sector_align(offsets[i] as u64, next_size as u64)
                .try_into()
                .unwrap();
            offsets[i] += adj;

            assert!(offsets[i] % 4 == 0);
        }

        let dirents = self.table.backing_vec().iter().map(|node| {
            (
                DirectoryEntryDiskNode {
                    left_entry_offset: node.left_idx().map(|idx| offsets[idx]).unwrap_or_default()
                        / 4,
                    right_entry_offset: node
                        .right_idx()
                        .map(|idx| offsets[idx])
                        .unwrap_or_default()
                        / 4,
                    dirent: node.data().node,
                },
                node.data().name,
            )
        });

        let mut dirent_bytes: Vec<u8> = Vec::new();
        let mut file_listing: Vec<FileListingEntry> = Vec::new();

        for (idx, (mut dirent, name)) in dirents.enumerate() {
            let sector = allocator.allocate_contiguous(dirent.dirent.data.size as u64);
            dirent.dirent.data.sector = sector.try_into().unwrap();

            file_listing.push(FileListingEntry {
                name: name.to_string(),
                sector,
                is_dir: dirent.dirent.attributes.directory(),
            });

            let bytes = dirent.serialize()?;
            let size = bytes.len() + dirent.dirent.filename_length as usize;
            assert_eq!(bytes.len(), 0xe);
            assert_eq!(
                name.as_bytes().len(),
                dirent.dirent.filename_length as usize
            );

            if dirent_bytes.len() < offsets[idx] as usize {
                let offset = offsets[idx] as usize;
                let diff = offset - dirent_bytes.len();
                dirent_bytes.extend_from_slice(&alloc::vec![0xff; diff]);
            }
            assert_eq!(dirent_bytes.len(), offsets[idx] as usize);

            dirent_bytes.extend_from_slice(&bytes);
            dirent_bytes.extend_from_slice(name.as_bytes());

            if size % 4 > 0 {
                let padding = 4 - size % 4;
                dirent_bytes.extend_from_slice(&alloc::vec![0xff; padding]);
            }
        }

        if let Some(size) = self.size {
            assert_eq!(dirent_bytes.len() as u64, size);
        } else {
            self.compute_size();
            assert_eq!(Some(dirent_bytes.len() as u64), self.size);
        }

        Ok(DirectoryEntryTableDiskRepr {
            entry_table: dirent_bytes.into_boxed_slice(),
            file_listing,
        })
    }
}
