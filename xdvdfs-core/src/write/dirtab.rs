use crate::layout::{self, DirectoryEntryData, DirectoryEntryDiskNode, DirentAttributes};
use crate::write::avl;

use alloc::string::ToString;
use alloc::{boxed::Box, string::String, vec::Vec};

use super::sector::{required_sectors, SectorAllocator};
use super::FileStructureError;

pub trait DirectoryEntryTableWriter {
    fn dirtab_size(&self) -> u32;
}

pub trait DirectoryEntryTableBuilder: Default {
    type DirtabWriter: DirectoryEntryTableWriter;

    fn add_dir(&mut self, name: &str, size: u32) -> Result<(), FileStructureError>;

    fn add_file(&mut self, name: &str, size: u32) -> Result<(), FileStructureError>;

    fn build(self) -> Result<Self::DirtabWriter, FileStructureError>;
}

#[derive(Default)]
pub struct AvlDirectoryEntryTableBuilder {
    table: avl::AvlTree<DirectoryEntryData>,
}

/// Writer for directory entry tables
pub struct AvlDirectoryEntryTableWriter {
    table: avl::AvlTree<DirectoryEntryData>,
    size: u32,
}

#[derive(Debug, Eq, PartialEq)]
pub struct FileListingEntry {
    pub name: String,
    pub sector: u64,
    pub size: u64,
    pub is_dir: bool,
}

pub struct DirectoryEntryTableDiskRepr {
    pub entry_table: Box<[u8]>,
    pub file_listing: Vec<FileListingEntry>,
}

/// Returns alignment needed to ensure an entry at a given sector offset and size
/// does not cross a sector boundary. If it does, the offset is aligned to the next sector.
fn sector_align(offset: u32, size: u32) -> u32 {
    let used_sectors = required_sectors(offset as u64);
    let needed_sectors = required_sectors(offset as u64 + size as u64);

    // If the offset already lands on a sector boundary, there is nothing to be done.
    // If we need more sectors to contain the data (offset + size) than everything up
    // to offset, then we know the data crosses a sector boundary.
    if offset % layout::SECTOR_SIZE > 0 && needed_sectors > used_sectors {
        offset.next_multiple_of(layout::SECTOR_SIZE) - offset
    } else {
        0
    }
}

/// Compute offsets from the start of the dirent sector
/// for each entry, given as an iterator of pairs, where the second
/// element is the file size of the entry.
/// The offset is a partial sum of entry sizes. The first element in the pair
/// allows the iterator to pass other items through for later use.
fn compute_offsets<T>(iter: impl Iterator<Item = (T, u32)>) -> impl Iterator<Item = (T, u32)> {
    iter.scan(0u32, |state, (val, file_size)| {
        // If this file does not fit at its offset, place it on a new sector
        *state += sector_align(*state, file_size);
        let curr_offset = *state;

        *state += file_size;
        Some((val, curr_offset))
    })
}

fn avl_index_to_offset(index: Option<usize>, offsets: &[u32]) -> Result<u16, FileStructureError> {
    index
        .map(|idx| offsets[idx] / 4)
        .unwrap_or_default()
        .try_into()
        .map_err(|_| FileStructureError::TooManyDirectoryEntries)
}

fn dirent_data_to_disk_node(
    data: &DirectoryEntryData,
    left_child_index: Option<usize>,
    right_child_index: Option<usize>,
    offsets: &[u32],
    name_bytes: &mut [u8],
    sector: u32,
) -> Result<DirectoryEntryDiskNode, FileStructureError> {
    let mut dirent = data.node;
    dirent.filename_length = data.encode_name(name_bytes)?;

    let left_entry_offset = avl_index_to_offset(left_child_index, offsets)?;
    let right_entry_offset = avl_index_to_offset(right_child_index, offsets)?;
    dirent.data.sector = sector;

    Ok(DirectoryEntryDiskNode {
        left_entry_offset,
        right_entry_offset,
        dirent,
    })
}

fn serialize_dirent_disk_node(
    table: &mut [u8],
    dirent: DirectoryEntryDiskNode,
    name_bytes: &[u8],
) -> Result<(), FileStructureError> {
    dirent
        .serialize_into(table)
        .map_err(|e| FileStructureError::SerializationError(e.into()))?;

    let name_len = dirent.dirent.filename_length as usize;
    table[0xe..(0xe + name_len)].copy_from_slice(&name_bytes[0..name_len]);

    Ok(())
}

impl AvlDirectoryEntryTableBuilder {
    fn add_node(
        &mut self,
        name: &str,
        size: u32,
        attributes: DirentAttributes,
    ) -> Result<(), FileStructureError> {
        let dirent = DirectoryEntryData::new_without_sector(name, size, attributes)?;

        self.table
            .insert(dirent)
            .then_some(())
            .ok_or(FileStructureError::DuplicateFileName)
    }
}

impl DirectoryEntryTableBuilder for AvlDirectoryEntryTableBuilder {
    type DirtabWriter = AvlDirectoryEntryTableWriter;

    fn add_dir(&mut self, name: &str, size: u32) -> Result<(), FileStructureError> {
        let attributes = DirentAttributes(0).with_directory(true);

        let size = size + ((2048 - size % 2048) % 2048);
        self.add_node(name, size, attributes)
    }

    fn add_file(&mut self, name: &str, size: u32) -> Result<(), FileStructureError> {
        let attributes = DirentAttributes(0).with_archive(true);
        self.add_node(name, size, attributes)
    }

    fn build(self) -> Result<AvlDirectoryEntryTableWriter, FileStructureError> {
        AvlDirectoryEntryTableWriter::new(self)
    }
}

impl DirectoryEntryTableWriter for AvlDirectoryEntryTableWriter {
    /// Returns the size of the directory entry table, in bytes.
    fn dirtab_size(&self) -> u32 {
        // FS bug: zero sized dirents are listed as size 2048
        if self.table.backing_vec().is_empty() {
            2048
        } else {
            self.size
        }
    }
}

impl AvlDirectoryEntryTableWriter {
    fn new(builder: AvlDirectoryEntryTableBuilder) -> Result<Self, FileStructureError> {
        let size = builder
            .table
            .preorder_iter()
            .map(|node| node.len_on_disk())
            .try_fold(0, |acc: u32, disk_len: Result<u32, FileStructureError>| {
                disk_len.map(|disk_len| acc + disk_len + sector_align(acc, disk_len))
            })?;
        Ok(Self {
            table: builder.table,
            size,
        })
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
    pub fn disk_repr(
        &self,
        allocator: &mut SectorAllocator,
    ) -> Result<DirectoryEntryTableDiskRepr, FileStructureError> {
        // Construct an array of offsets for each entry in the table
        // Each offset is a partial sum of lengths of the dirent on disk,
        // computed in preorder, then unmapped to backing order.
        // Note that the len_on_disk is already known to be valid at this point,
        // since the size has been computed in `new()` and would use the same
        // mechanism.
        let mut avl_idx_to_dirtab_offset = alloc::vec![0u32; self.table.len()];
        let mut avl_idx_to_sector = alloc::vec![0u32; self.table.len()];
        let preorder_idx_file_size_iter = self.table.preorder_iter().map(|node| {
            (
                (node.backing_index(), node.node.data.size),
                node.len_on_disk().unwrap_or(0),
            )
        });
        for ((backing_idx, size), offset) in compute_offsets(preorder_idx_file_size_iter) {
            avl_idx_to_dirtab_offset[backing_idx] = offset;
            avl_idx_to_sector[backing_idx] = allocator.allocate_contiguous(size as u64);
        }

        let size = self.dirtab_size().next_multiple_of(layout::SECTOR_SIZE) as usize;

        let mut dirent_bytes = alloc::vec![0xffu8; size];
        let mut file_listing: Vec<FileListingEntry> = Vec::with_capacity(self.table.len());

        for (idx, node) in self.table.backing_vec().iter().enumerate() {
            let mut name_bytes = [0; 256];
            let dirent = dirent_data_to_disk_node(
                node.data(),
                node.left_idx(),
                node.right_idx(),
                &avl_idx_to_dirtab_offset,
                &mut name_bytes,
                avl_idx_to_sector[idx],
            )?;

            file_listing.push(FileListingEntry {
                name: node.data().name_str().to_string(),
                sector: dirent.dirent.data.sector as u64,
                size: dirent.dirent.data.size as u64,
                is_dir: dirent.dirent.attributes.directory(),
            });

            let offset = avl_idx_to_dirtab_offset[idx] as usize;
            serialize_dirent_disk_node(&mut dirent_bytes[offset..], dirent, &name_bytes)?;
        }

        Ok(DirectoryEntryTableDiskRepr {
            entry_table: dirent_bytes.into_boxed_slice(),
            file_listing,
        })
    }
}

#[cfg(test)]
mod test {
    use std::string::{String, ToString};

    use crate::{
        layout::DirectoryEntryDiskNode,
        write::{dirtab::FileListingEntry, sector::SectorAllocator, FileStructureError},
    };
    use alloc::vec::Vec;

    use super::{
        avl_index_to_offset, compute_offsets, dirent_data_to_disk_node, sector_align,
        serialize_dirent_disk_node, AvlDirectoryEntryTableBuilder, DirectoryEntryTableBuilder,
        DirectoryEntryTableWriter,
    };

    #[test]
    fn test_sector_align_offset_aligned() {
        assert_eq!(sector_align(0, 20), 0);
        assert_eq!(sector_align(2048, 20), 0);
    }

    #[test]
    fn test_sector_align_offset_contained_in_sector() {
        assert_eq!(sector_align(1000, 50), 0);
        assert_eq!(sector_align(2000, 20), 0);
    }

    #[test]
    fn test_sector_align_crosses_boundary() {
        assert_eq!(sector_align(2040, 20), 8);
        assert_eq!(sector_align(2020, 40), 28);
    }

    #[test]
    fn test_compute_offsets_single_sector() {
        let sizes: &mut [u32] = &mut [100, 100, 100, 100];
        let offsets: Vec<u32> = compute_offsets(sizes.iter().map(|x| ((), *x)))
            .map(|x| x.1)
            .collect();
        assert_eq!(offsets, &[0, 100, 200, 300,]);
    }

    #[test]
    fn test_compute_offsets_multiple_sectors() {
        let sizes: &mut [u32] = &mut [252, 252, 252, 252, 252, 252, 252, 252, 252, 252];
        let offsets: Vec<u32> = compute_offsets(sizes.iter().map(|x| ((), *x)))
            .map(|x| x.1)
            .collect();
        assert_eq!(
            offsets,
            &[
                0, 252, 504, 756, 1008, 1260, 1512, 1764,
                // 252 no longer fits in sector, push to 2048
                2048, 2300,
            ]
        );
    }

    #[test]
    fn test_avl_index_to_offset_leaf() {
        let offsets = &[0, 100, 200];
        assert_eq!(avl_index_to_offset(None, offsets), Ok(0));
    }

    #[test]
    fn test_avl_index_to_offset_node() {
        let offsets = &[0, 100, 200];
        assert_eq!(avl_index_to_offset(Some(1), offsets), Ok(25));
    }

    #[test]
    fn test_avl_index_to_offset_out_of_range() {
        use crate::write::FileStructureError;

        let offsets = &[0, 100, 262144];
        assert_eq!(
            avl_index_to_offset(Some(2), offsets),
            Err(FileStructureError::TooManyDirectoryEntries)
        );
    }

    #[test]
    fn test_dirent_data_to_disk_node_leaf_node() {
        use crate::layout::{DirectoryEntryData, DirentAttributes};

        let mut name_bytes = [0; 256];
        let data = DirectoryEntryData::new_without_sector(
            "HelloWorld",
            2048,
            DirentAttributes(0).with_directory(true),
        )
        .expect("Data should be valid");
        let offsets = &[0, 2048, 4096];

        let node = dirent_data_to_disk_node(&data, None, None, offsets, &mut name_bytes, 33)
            .expect("Node should be created without error");
        assert_eq!({ node.left_entry_offset }, 0);
        assert_eq!({ node.right_entry_offset }, 0);
        assert_eq!({ node.dirent.data.sector }, 33);
        assert_eq!({ node.dirent.filename_length }, 10);
        assert_eq!(&name_bytes[0..10], "HelloWorld".as_bytes());
    }

    #[test]
    fn test_dirent_data_to_disk_node_with_child_nodes() {
        use crate::layout::{DirectoryEntryData, DirentAttributes};

        let mut name_bytes = [0; 256];
        let data = DirectoryEntryData::new_without_sector(
            "HelloWorld",
            2048,
            DirentAttributes(0).with_directory(true),
        )
        .expect("Data should be valid");
        let offsets = &[0, 2048, 4096];

        let node = dirent_data_to_disk_node(&data, Some(1), Some(2), offsets, &mut name_bytes, 33)
            .expect("Node should be created without error");
        assert_eq!({ node.left_entry_offset }, 512);
        assert_eq!({ node.right_entry_offset }, 1024);
        assert_eq!({ node.dirent.data.sector }, 33);
        assert_eq!({ node.dirent.filename_length }, 10);
        assert_eq!(&name_bytes[0..10], "HelloWorld".as_bytes());
    }

    #[test]
    fn test_serialize_dirent_disk_node_without_alignment() {
        use crate::layout::{DirectoryEntryDiskData, DirentAttributes, DiskRegion};
        let node = DirectoryEntryDiskNode {
            left_entry_offset: 512,
            right_entry_offset: 1024,
            dirent: DirectoryEntryDiskData {
                data: DiskRegion {
                    sector: 33,
                    size: 2048,
                },
                attributes: DirentAttributes(0).with_directory(true),
                filename_length: 10,
            },
        };
        let name_bytes = "HelloWorld".as_bytes();

        // Initial len + sizeof(node) + sizeof(name)
        let mut buffer = alloc::vec![0xff; 2048 + 0xe + 10];
        assert!(serialize_dirent_disk_node(&mut buffer[2048..], node, name_bytes).is_ok());
        assert_eq!(&buffer[0..2048], alloc::vec![0xff; 2048]);
        assert_eq!(&buffer[2048..(2048 + 0xe)], node.serialize().unwrap());
        assert_eq!(&buffer[(2048 + 0xe)..], name_bytes);
    }

    #[test]
    fn test_serialize_dirent_disk_node_with_alignment() {
        use crate::layout::{DirectoryEntryDiskData, DirentAttributes, DiskRegion};
        let node = DirectoryEntryDiskNode {
            left_entry_offset: 512,
            right_entry_offset: 1024,
            dirent: DirectoryEntryDiskData {
                data: DiskRegion {
                    sector: 33,
                    size: 2048,
                },
                attributes: DirentAttributes(0).with_directory(true),
                filename_length: 11,
            },
        };
        let name_bytes = "HelloWorlds".as_bytes();

        // Initial len + sizeof(node) + sizeof(name) + 4-byte alignment
        let mut buffer = alloc::vec![0xff; 2048 + 0xe + 11 + 3];
        assert!(serialize_dirent_disk_node(&mut buffer[2048..], node, name_bytes).is_ok());
        assert_eq!(&buffer[0..2048], alloc::vec![0xff; 2048]);
        assert_eq!(&buffer[2048..(2048 + 0xe)], node.serialize().unwrap());
        assert_eq!(&buffer[(2048 + 0xe)..(2048 + 0xe + 11)], name_bytes);
        assert_eq!(&buffer[(2048 + 0xe + 11)..], &[0xff, 0xff, 0xff]);
    }

    #[test]
    fn test_dirtab_writer_empty_size_computation() {
        let writer = AvlDirectoryEntryTableBuilder::default();
        let writer = writer.build().expect("Directory should be valid");

        assert_eq!(writer.dirtab_size(), 2048);
    }

    #[test]
    fn test_dirtab_writer_single_directory_size_computation() {
        let mut writer = AvlDirectoryEntryTableBuilder::default();
        assert_eq!(writer.add_dir("test", 30), Ok(()));

        let writer = writer.build().expect("Directory should be valid");
        assert_eq!(writer.dirtab_size(), 0xe + 4 + 2);
    }

    #[test]
    fn test_dirtab_writer_single_file_size_computation() {
        let mut writer = AvlDirectoryEntryTableBuilder::default();
        assert_eq!(writer.add_file("test", 30), Ok(()));

        let writer = writer.build().expect("Directory should be valid");
        assert_eq!(writer.dirtab_size(), 0xe + 4 + 2);
    }

    #[test]
    fn test_dirtab_writer_multiple_entry_size_computation_without_realignment() {
        let mut writer = AvlDirectoryEntryTableBuilder::default();
        assert_eq!(writer.add_file("file", 30), Ok(()));
        assert_eq!(writer.add_dir("dir", 30), Ok(()));

        let writer = writer.build().expect("Directory should be valid");

        let dirent_len = 2 * 0xe;
        let filename_len = 4 + 3;
        let padding = 2 + 3;
        assert_eq!(writer.dirtab_size(), dirent_len + filename_len + padding);
    }

    #[test]
    fn test_dirtab_writer_multiple_entry_size_computation_with_realignment() {
        let mut writer = AvlDirectoryEntryTableBuilder::default();

        // Add 103 files with name length '6'
        // Each dirent is of length 20, with a total over 2048
        for i in 0..103 {
            assert_eq!(writer.add_file(&alloc::format!("{i:06}"), 10), Ok(()));
        }

        let writer = writer.build().expect("Directory should be valid");

        let dirent_len = 103 * 0xe;
        let filename_len = 103 * 6;
        let alignment = 8; // 102 * 20 = 2040, align to 2048
        assert_eq!(writer.dirtab_size(), dirent_len + filename_len + alignment);
    }

    #[test]
    fn test_dirtab_writer_serialize_empty_directory() {
        let writer = AvlDirectoryEntryTableBuilder::default();
        let writer = writer.build().expect("Directory should be valid");
        let mut allocator = SectorAllocator::default();

        let repr = writer
            .disk_repr(&mut allocator)
            .expect("Dirtab serialization should be valid");

        // Empty tables are '0xff' filled
        assert_eq!(repr.entry_table.as_ref(), &alloc::vec![0xff; 2048]);
        assert!(repr.file_listing.is_empty());
    }

    #[test]
    fn test_dirtab_writer_serialize_single_file() {
        let mut writer = AvlDirectoryEntryTableBuilder::default();
        assert_eq!(writer.add_file("test", 10), Ok(()));

        let writer = writer.build().expect("Directory should be valid");
        let mut allocator = SectorAllocator::default();

        let repr = writer
            .disk_repr(&mut allocator)
            .expect("Dirtab serialization should be valid");

        assert_eq!(
            &repr.entry_table[0..0xe],
            &[0, 0, 0, 0, 33, 0, 0, 0, 10, 0, 0, 0, 32, 4,]
        );
        assert_eq!(&repr.entry_table[0xe..(0xe + 4)], "test".as_bytes());
        assert_eq!(&repr.entry_table[(0xe + 4)..], &alloc::vec![0xff; 2030]);
        assert_eq!(
            repr.file_listing,
            &[FileListingEntry {
                name: "test".to_string(),
                sector: 33,
                size: 10,
                is_dir: false,
            },]
        );
    }

    #[test]
    fn test_dirtab_writer_serialize_single_dir() {
        let mut writer = AvlDirectoryEntryTableBuilder::default();
        assert_eq!(writer.add_dir("test", 20), Ok(()));

        let writer = writer.build().expect("Directory should be valid");
        let mut allocator = SectorAllocator::default();
        let repr = writer
            .disk_repr(&mut allocator)
            .expect("Dirtab serialization should be valid");

        assert_eq!(
            &repr.entry_table[0..0xe],
            &[
                0, 0, 0, 0, 33, 0, 0, 0, 0, 8, 0, 0, // Dir is padded up to 2048 in size
                16, 4,
            ]
        );
        assert_eq!(&repr.entry_table[0xe..(0xe + 4)], "test".as_bytes());
        assert_eq!(&repr.entry_table[(0xe + 4)..], &alloc::vec![0xff; 2030]);
        assert_eq!(
            repr.file_listing,
            &[FileListingEntry {
                name: "test".to_string(),
                sector: 33,
                size: 2048,
                is_dir: true,
            },]
        );
    }

    #[test]
    fn test_dirtab_writer_serialize_tree_entries() {
        let mut writer = AvlDirectoryEntryTableBuilder::default();
        assert_eq!(writer.add_dir("t1", 20), Ok(()));
        assert_eq!(writer.add_dir("t2", 20), Ok(()));
        assert_eq!(writer.add_dir("t3", 20), Ok(()));

        let writer = writer.build().expect("Directory should be valid");
        let mut allocator = SectorAllocator::default();
        let repr = writer
            .disk_repr(&mut allocator)
            .expect("Dirtab serialization should be valid");

        let entry_size: usize = 0xe + 2;

        assert_eq!(&repr.entry_table[0..4], &[4, 0, 8, 0]);
        assert_eq!(
            &repr.entry_table[entry_size..(entry_size + 4)],
            &[0, 0, 0, 0]
        );
        assert_eq!(
            &repr.entry_table[(2 * entry_size)..(2 * entry_size + 4)],
            &[0, 0, 0, 0]
        );
        assert_eq!(
            repr.file_listing,
            &[
                FileListingEntry {
                    name: "t1".to_string(),
                    sector: 34,
                    size: 2048,
                    is_dir: true,
                },
                FileListingEntry {
                    name: "t2".to_string(),
                    sector: 33,
                    size: 2048,
                    is_dir: true,
                },
                FileListingEntry {
                    name: "t3".to_string(),
                    sector: 35,
                    size: 2048,
                    is_dir: true,
                },
            ]
        );
    }

    #[test]
    fn test_dirtab_writer_serialize_entry_sector_alignment() {
        let mut writer = AvlDirectoryEntryTableBuilder::default();

        // Add 103 files with name length '6'
        // Each dirent is of length 20, with a total over 2048
        for i in 0..103 {
            assert_eq!(writer.add_file(&alloc::format!("{i:06}"), 10), Ok(()));
        }

        let writer = writer.build().expect("Directory should be valid");
        let mut allocator = SectorAllocator::default();
        let repr = writer
            .disk_repr(&mut allocator)
            .expect("Dirtab serialization should be valid");

        let entry_size: usize = 0xe + 6;
        let aligned_entry_offset: usize = entry_size * 102;
        assert_eq!(
            &repr.entry_table[aligned_entry_offset..(aligned_entry_offset + 8)],
            &alloc::vec![0xff; 8]
        );
    }

    #[test]
    fn test_dirtab_writer_reject_duplicate_names() {
        let mut writer = AvlDirectoryEntryTableBuilder::default();
        assert_eq!(writer.add_file("t1", 10), Ok(()));
        assert_eq!(
            writer.add_dir("t1", 20),
            Err(FileStructureError::DuplicateFileName)
        );
    }

    #[test]
    fn test_dirtab_writer_reject_long_name() {
        let long_name: String = core::iter::repeat_n('a', 260).collect();
        let mut writer = AvlDirectoryEntryTableBuilder::default();
        assert_eq!(
            writer.add_dir(&long_name, 20),
            Err(FileStructureError::FileNameTooLong)
        );
    }
}
