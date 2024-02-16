use core::fmt::Display;

use super::util;
use bincode::Options;
use encoding_rs::WINDOWS_1252;
use proc_bitfield::bitfield;
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

use maybe_async::maybe_async;

pub const SECTOR_SIZE: u32 = 2048;
pub const VOLUME_HEADER_MAGIC: [u8; 0x14] = *b"MICROSOFT*XBOX*MEDIA";

/// Represents a contiguous region on the disk image, given by sector number and
/// size.
#[repr(C)]
#[repr(packed)]
#[derive(Deserialize, Serialize, Debug, Copy, Clone, Eq, PartialEq)]
pub struct DiskRegion {
    pub sector: u32,
    pub size: u32,
}

/// A DiskRegion that contains a directory entry table structure.
///
/// This differentiates regions that contain file data.
#[repr(C)]
#[repr(packed)]
#[derive(Deserialize, Serialize, Debug, Copy, Clone)]
pub struct DirectoryEntryTable {
    pub region: DiskRegion,
}

/// XDVDFS volume information, located at sector 32 on the disk
#[repr(C)]
#[repr(packed)]
#[derive(Deserialize, Serialize, Debug, Copy, Clone)]
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
#[derive(Deserialize, Serialize, Copy, Clone, Eq, PartialEq)]
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
#[derive(Deserialize, Serialize, Debug, Copy, Clone)]
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
#[derive(Deserialize, Serialize, Debug, Copy, Clone, Eq, PartialEq)]
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
    pub offset: u64,
}

/// In-memory structure to contain the on-disk dirent data,
/// and file name information.
///
/// This does not contain information about on-disk left or
/// right subtrees.
///
/// Intended use is for building the dirent tree within some other
/// data structure, and then creating the on-disk structure separately
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg(feature = "write")]
pub struct DirectoryEntryData {
    pub node: DirectoryEntryDiskData,
    pub name: arrayvec::ArrayString<256>,
}

impl DiskRegion {
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn offset<E>(&self, offset: u32) -> Result<u64, util::Error<E>> {
        if offset >= self.size {
            return Err(util::Error::SizeOutOfBounds(offset, self.size));
        }

        let offset = SECTOR_SIZE as u64 * self.sector as u64 + offset as u64;
        Ok(offset)
    }
}

impl DirectoryEntryTable {
    pub fn new(size: u32, sector: u32) -> Self {
        Self {
            region: DiskRegion { size, sector },
        }
    }

    pub fn is_empty(&self) -> bool {
        self.region.is_empty()
    }

    pub fn offset<E>(&self, offset: u32) -> Result<u64, util::Error<E>> {
        self.region.offset(offset)
    }
}

impl VolumeDescriptor {
    pub fn new(root_table: DirectoryEntryTable) -> Self {
        Self {
            magic0: VOLUME_HEADER_MAGIC,
            root_table,
            filetime: 0,
            unused: [0; 0x7c8],
            magic1: VOLUME_HEADER_MAGIC,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.magic0 == VOLUME_HEADER_MAGIC && self.magic1 == VOLUME_HEADER_MAGIC
    }

    pub fn serialize<E>(&self) -> Result<alloc::vec::Vec<u8>, util::Error<E>> {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .serialize(self)
            .map_err(|e| util::Error::SerializationFailed(e))
    }

    pub fn deserialize<E>(buf: &[u8; 0x800]) -> Result<Self, util::Error<E>> {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .deserialize(buf)
            .map_err(|e| util::Error::SerializationFailed(e))
    }
}

impl Display for DirentAttributes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use alloc::vec::Vec;
        let mut attrs: Vec<&str> = Vec::new();

        if self.directory() {
            attrs.push("Directory");
        }

        if self.read_only() {
            attrs.push("Read-Only");
        }

        if self.hidden() {
            attrs.push("Hidden");
        }

        if self.system() {
            attrs.push("System");
        }

        if self.archive() {
            attrs.push("Archive");
        }

        if self.normal() {
            attrs.push("Normal");
        }

        let attrs = attrs.join(" ");
        f.write_str(&attrs)
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
    #[maybe_async]
    pub async fn read_data<E>(
        &self,
        dev: &mut impl super::blockdev::BlockDeviceRead<E>,
        buf: &mut [u8],
    ) -> Result<(), util::Error<E>> {
        if self.data.size == 0 {
            return Ok(());
        }

        let offset = self.data.offset(0)?;
        dev.read(offset, buf)
            .await
            .map_err(|e| util::Error::IOError(e))?;
        Ok(())
    }

    #[cfg(feature = "read")]
    #[maybe_async]
    pub async fn read_data_all<E>(
        &self,
        dev: &mut impl super::blockdev::BlockDeviceRead<E>,
    ) -> Result<alloc::boxed::Box<[u8]>, util::Error<E>> {
        let buf = alloc::vec![0; self.data.size as usize];
        let mut buf = buf.into_boxed_slice();

        if self.data.size == 0 {
            return Ok(buf);
        }

        let offset = self.data.offset(0)?;
        dev.read(offset, &mut buf)
            .await
            .map_err(|e| util::Error::IOError(e))?;

        Ok(buf)
    }

    #[cfg(all(feature = "read", feature = "std"))]
    pub fn seek_to(
        &self,
        seek: &mut impl std::io::Seek,
    ) -> Result<(), util::Error<std::io::Error>> {
        use std::io::SeekFrom;

        let offset = self.data.offset(0)?;
        seek.seek(SeekFrom::Start(offset))?;
        Ok(())
    }
}

impl DirectoryEntryNode {
    pub fn name_slice(&self) -> &[u8] {
        let name_len = self.node.dirent.filename_length as usize;
        &self.name[0..name_len]
    }

    /// Returns a UTF-8 encoded representation of the file name
    /// If the filename cannot be reencoded into UTF-8, returns None
    pub fn name_str<E>(&self) -> Result<alloc::borrow::Cow<str>, util::Error<E>> {
        let name_bytes = self.name_slice();
        WINDOWS_1252
            .decode_without_bom_handling_and_without_replacement(name_bytes)
            .ok_or(util::Error::StringEncodingError)
    }
}

#[cfg(feature = "write")]
impl DirectoryEntryData {
    pub fn name_slice(&self) -> &[u8] {
        let name_len = self.node.filename_length as usize;
        &self.name.as_str().as_bytes()[0..name_len]
    }

    pub fn name_str(&self) -> &str {
        self.name.as_str()
    }

    pub fn encode_name<E>(&self, buffer: &mut [u8]) -> Result<u8, util::Error<E>> {
        let mut encoder = WINDOWS_1252.new_encoder();
        let (result, bytes_read, bytes_written) =
            encoder.encode_from_utf8_without_replacement(self.name_str(), buffer, true);
        match result {
            encoding_rs::EncoderResult::InputEmpty => {}
            _ => return Err(util::Error::StringEncodingError),
        }
        if bytes_read != self.name.len() {
            Err(util::Error::StringEncodingError)
        } else {
            TryInto::<u8>::try_into(bytes_written).map_err(|_| util::Error::StringEncodingError)
        }
    }

    /// Returns the length (in bytes) of the directory entry
    /// on disk, after serialization
    pub fn len_on_disk<E>(&self) -> Result<u32, util::Error<E>> {
        let encoded_filename_len = self.encode_name(&mut [0; 256])?;
        let mut size = 0xe + (encoded_filename_len as u32);

        if size % 4 > 0 {
            size += 4 - size % 4;
        }

        Ok(size)
    }
}

#[cfg(feature = "write")]
impl PartialOrd for DirectoryEntryData {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(feature = "write")]
impl Ord for DirectoryEntryData {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        util::cmp_ignore_case_utf8(self.name_str(), other.name_str())
    }
}

impl DirectoryEntryDiskNode {
    pub fn serialize<E>(&self) -> Result<alloc::vec::Vec<u8>, util::Error<E>> {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .serialize(self)
            .map_err(|e| util::Error::SerializationFailed(e))
    }

    pub fn deserialize<E>(buf: &[u8; 0xe]) -> Result<Self, util::Error<E>> {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .deserialize(buf)
            .map_err(|e| util::Error::SerializationFailed(e))
    }
}

#[cfg(test)]
mod test {
    use super::{DirectoryEntryDiskData, DirentAttributes, DiskRegion};
    use futures::executor;

    #[test]
    fn test_read_file_empty() {
        let mut data: [u8; 8] = [0; 8];

        let dirent = DirectoryEntryDiskData {
            data: DiskRegion { sector: 0, size: 0 },
            attributes: DirentAttributes(0),
            filename_length: 0,
        };

        let mut buf = [0; 8];
        executor::block_on(dirent.read_data(&mut data, &mut buf)).unwrap();
    }

    #[test]
    fn test_read_file_all_empty() {
        let mut data: [u8; 8] = [0; 8];

        let dirent = DirectoryEntryDiskData {
            data: DiskRegion { sector: 0, size: 0 },
            attributes: DirentAttributes(0),
            filename_length: 0,
        };

        let data = executor::block_on(dirent.read_data_all(&mut data)).unwrap();
        assert_eq!(data.len(), 0);
    }
}
