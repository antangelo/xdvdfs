use proc_bitfield::bitfield;
use serde::Deserialize;
use serde_big_array::BigArray;

pub const SECTOR_SIZE: usize = 2048;
pub const VOLUME_HEADER_MAGIC: &[u8] = "MICROSOFT*XBOX*MEDIA".as_bytes();

/// Represents a contiguous region on the disk image, given by sector number and
/// size.
#[repr(C)]
#[repr(packed)]
#[derive(Deserialize, Debug, Copy, Clone)]
pub struct DiskRegion {
    pub sector: u32,
    pub size: u32,
}

/// A DiskRegion that contains a directory entry table structure.
///
/// This differentiates regions that contain file data.
#[repr(C)]
#[repr(packed)]
#[derive(Deserialize, Debug, Copy, Clone)]
pub struct DirectoryEntryTable {
    pub region: DiskRegion,
}

/// XDVDFS volume information, located at sector 32 on the disk
#[repr(C)]
#[repr(packed)]
#[derive(Deserialize, Debug, Copy, Clone)]
pub struct VolumeDescriptor {
    magic0: [u8; 0x14],
    pub root_table: DirectoryEntryTable,
    pub filetime: u64,

    #[serde(with = "BigArray")]
    unused: [u8; 0x7c8],

    magic1: [u8; 0x14],
}

bitfield!(
#[repr(C)]
#[derive(Deserialize, Copy, Clone)]
pub struct DirentAttributes(pub u8): Debug {
    pub attrs: u8 @ ..,

    pub read_only: bool @ 0,
    pub hidden: bool @ 1,
    pub system: bool @ 2,
    pub directory: bool @ 4,
    pub archive: bool @ 5,
    pub normal: bool @ 7,
}
);

/// On-disk representation of a directory entry tree node,
/// including the left and right children, and data.
///
/// Does not include the file name or padding.
#[repr(C)]
#[repr(packed)]
#[derive(Deserialize, Debug, Copy, Clone)]
pub struct DirectoryEntryDiskNode {
    pub left_entry_offset: u16,
    pub right_entry_offset: u16,
    pub dirent: DirectoryEntryDiskData,
}

/// On-disk representation of a directory entry tree data,
/// excluding the left and right children.
///
/// Does not include the file name or padding.
#[repr(C)]
#[repr(packed)]
#[derive(Deserialize, Debug, Copy, Clone)]
pub struct DirectoryEntryDiskData {
    pub data: DiskRegion,
    pub attributes: DirentAttributes,
    pub filename_length: u8,
}

/// In-memory structure to contain an on-disk tree node and
/// file name information.
#[derive(Debug, Copy, Clone)]
pub struct DirectoryEntryNode {
    pub node: DirectoryEntryDiskNode,
    pub name: [u8; 256],
}

/// In-memory structure to contain the on-disk dirent data,
/// and file name information.
///
/// This does not contain information about on-disk left or
/// right subtrees.
///
/// Intended use is for building the dirent tree within some other
/// data structure, and then creating the on-disk structure separately
#[derive(Debug, Copy, Clone)]
pub struct DirectoryEntryData {
    pub node: DirectoryEntryDiskData,
    pub name: [u8; 256],
}

impl DiskRegion {
    pub fn is_empty(&self) -> bool {
        self.sector == 0 && self.size == 0
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn offset(&self, offset: u32) -> Option<usize> {
        if offset >= self.size {
            return None;
        }

        let offset = SECTOR_SIZE * self.sector as usize + offset as usize;
        Some(offset)
    }
}

impl DirectoryEntryTable {
    pub fn is_empty(&self) -> bool {
        self.region.is_empty()
    }

    pub fn offset(&self, offset: u32) -> Option<usize> {
        self.region.offset(offset)
    }
}

impl VolumeDescriptor {
    pub fn is_valid(&self) -> bool {
        let header: &[u8; 0x14] = VOLUME_HEADER_MAGIC.try_into().unwrap();
        self.magic0 == *header && self.magic1 == *header
    }
}

impl DirectoryEntryDiskData {
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn is_directory(&self) -> bool {
        self.attributes.directory()
    }

    pub fn dirent_table(&self) -> Option<DirectoryEntryTable> {
        if self.is_directory() {
            Some(DirectoryEntryTable { region: self.data })
        } else {
            None
        }
    }

    #[cfg(feature = "read")]
    pub fn read_data(&self, dev: &mut impl super::blockdev::BlockDeviceRead, buf: &mut [u8]) {
        let offset = self.data.offset(0).unwrap();

        dev.read(offset, buf);
    }

    #[cfg(all(feature = "read", feature = "alloc"))]
    pub fn read_data_all(
        &self,
        dev: &mut impl super::blockdev::BlockDeviceRead,
    ) -> alloc::boxed::Box<[u8]> {
        use alloc::vec::Vec;

        let mut buf = Vec::new();
        buf.resize(self.data.size() as usize, 0);
        let mut buf = buf.into_boxed_slice();

        let offset = self.data.offset(0).unwrap();
        dev.read(offset, &mut buf);

        buf
    }
}

impl DirectoryEntryNode {
    pub fn name_slice(&self) -> &[u8] {
        let name_len = self.node.dirent.filename_length as usize;
        &self.name[0..name_len]
    }

    #[cfg(feature = "alloc")]
    pub fn get_name(&self) -> alloc::string::String {
        use alloc::string::String;
        String::from_utf8_lossy(self.name_slice()).into_owned()
    }
}

pub fn cmp_ignore_case_utf8(a: &str, b: &str) -> core::cmp::Ordering {
    use core::cmp::Ordering;
    use itertools::{EitherOrBoth, Itertools};

    a.chars()
        .flat_map(char::to_lowercase)
        .zip_longest(b.chars().flat_map(char::to_lowercase))
        .map(|ab| match ab {
            EitherOrBoth::Left(_) => Ordering::Greater,
            EitherOrBoth::Right(_) => Ordering::Less,
            EitherOrBoth::Both(a, b) => a.cmp(&b),
        })
        .find(|&ordering| ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}
