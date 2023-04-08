use super::blockdev::BlockDeviceRead;
use super::layout::{
    self, DirectoryEntryDiskNode, DirectoryEntryNode, DirectoryEntryTable, VolumeDescriptor,
};

use bincode::Options;

/// Read the XDVDFS volume descriptor from sector 32 of the drive
/// Returns None if the volume descriptor is invalid
pub fn read_volume(dev: &mut impl BlockDeviceRead) -> Option<VolumeDescriptor> {
    let mut buffer = [0; core::mem::size_of::<VolumeDescriptor>()];
    dev.read(layout::SECTOR_SIZE * 32, &mut buffer);

    let volume: VolumeDescriptor = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .deserialize(&buffer)
        .ok()?;
    if volume.is_valid() {
        Some(volume)
    } else {
        None
    }
}

fn read_dirent(dev: &mut impl BlockDeviceRead, offset: usize) -> Option<DirectoryEntryNode> {
    let mut dirent_buf = [0; 0xe];
    dev.read(offset, &mut dirent_buf);

    let node: DirectoryEntryDiskNode = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .deserialize(&dirent_buf)
        .ok()?;

    let mut dirent = DirectoryEntryNode {
        node,
        name: [0; 256],
    };

    let name_len = dirent.node.dirent.filename_length as usize;
    let name_buf = &mut dirent.name[0..name_len];
    dev.read(offset + 0xe, name_buf);

    Some(dirent)
}

impl VolumeDescriptor {
    pub fn root_dirent(&self, dev: &mut impl BlockDeviceRead) -> Option<DirectoryEntryNode> {
        if self.root_table.is_empty() {
            return None;
        }

        read_dirent(dev, self.root_table.offset(0)?)
    }
}

impl DirectoryEntryTable {
    fn find_dirent(
        &self,
        dev: &mut impl BlockDeviceRead,
        name: &str,
    ) -> Option<DirectoryEntryNode> {
        let mut offset = self.offset(0)?;

        loop {
            let dirent = read_dirent(dev, offset)?;
            dprintln!(
                "[find_dirent] Found {}: {:?}",
                dirent.get_name(),
                dirent.node
            );
            let dirent_name = core::str::from_utf8(dirent.name_slice()).ok()?;
            dprintln!("[find_dirent] Parsed name: {}", dirent_name);

            let cmp = layout::cmp_ignore_case_utf8(name, dirent_name);

            let next_offset = match cmp {
                core::cmp::Ordering::Equal => return Some(dirent),
                core::cmp::Ordering::Less => dirent.node.left_entry_offset,
                core::cmp::Ordering::Greater => dirent.node.right_entry_offset,
            };

            if next_offset == 0 {
                return None;
            }

            offset = self.offset(4 * next_offset as u32)?;
        }
    }

    /// Retrieves the directory entry node corresponding to the provided path,
    /// if it exists.
    ///
    /// Returns None if the root path is provided (root has no dirent)
    /// or the path does not exist.
    pub fn walk_path(
        &self,
        dev: &mut impl BlockDeviceRead,
        path: &str,
    ) -> Option<DirectoryEntryNode> {
        let mut dirent_tab = *self;
        let mut path_iter = path
            .trim_start_matches('/')
            .split_terminator('/')
            .peekable();

        while let Some(segment) = path_iter.next() {
            let dirent = dirent_tab.find_dirent(dev, segment)?;
            dprintln!("[walk_path] Got dirent: {:?}", dirent.node);
            let dirent_data = &dirent.node.dirent;

            if path_iter.peek().is_none() {
                return Some(dirent);
            }

            dirent_tab = dirent_data.dirent_table()?;
        }

        None
    }

    /// Walks the directory entry table in preorder, returning all directory entries.
    #[cfg(feature = "alloc")]
    pub fn walk_dirent_tree(
        &self,
        dev: &mut impl BlockDeviceRead,
    ) -> alloc::vec::Vec<DirectoryEntryNode> {
        use alloc::vec;

        let mut dirents = vec![];
        if self.is_empty() {
            return dirents;
        }

        let mut stack = vec![0];
        while let Some(top) = stack.pop() {
            let offset = self.offset(top).unwrap();
            let dirent = read_dirent(dev, offset).unwrap();

            if dirent.node.left_entry_offset != 0 {
                stack.push(4 * dirent.node.left_entry_offset as u32);
            }

            if dirent.node.right_entry_offset != 0 {
                stack.push(4 * dirent.node.right_entry_offset as u32);
            }

            dirents.push(dirent);
        }

        dirents
    }

    #[cfg(feature = "alloc")]
    pub fn file_tree(
        &self,
        dev: &mut impl BlockDeviceRead,
    ) -> alloc::vec::Vec<(alloc::string::String, DirectoryEntryNode)> {
        use alloc::format;
        use alloc::string::String;
        use alloc::vec;

        let mut dirents = vec![];

        let mut stack = vec![(String::from(""), *self)];
        while let Some((parent, tree)) = stack.pop() {
            let children = tree.walk_dirent_tree(dev);
            for child in children.iter() {
                if let Some(dirent_table) = child.node.dirent.dirent_table() {
                    let child_name = core::str::from_utf8(child.name_slice()).unwrap();
                    stack.push((format!("{}/{}", parent, child_name), dirent_table));
                } else {
                    dirents.push((parent.clone(), *child));
                }
            }
        }

        dirents
    }
}
