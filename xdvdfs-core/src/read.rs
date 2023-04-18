use super::blockdev::BlockDeviceRead;
use super::layout::{
    self, DirectoryEntryDiskNode, DirectoryEntryNode, DirectoryEntryTable, VolumeDescriptor,
};
use super::util;

/// Read the XDVDFS volume descriptor from sector 32 of the drive
/// Returns None if the volume descriptor is invalid
pub fn read_volume<E>(
    dev: &mut impl BlockDeviceRead<E>,
) -> Result<VolumeDescriptor, util::Error<E>> {
    let mut buffer = [0; core::mem::size_of::<VolumeDescriptor>()];
    dev.read(layout::SECTOR_SIZE * 32, &mut buffer)
        .map_err(|e| util::Error::IOError(e))?;

    let volume = VolumeDescriptor::deserialize(&buffer)?;
    if volume.is_valid() {
        Ok(volume)
    } else {
        Err(util::Error::InvalidVolume)
    }
}

fn read_dirent<E>(
    dev: &mut impl BlockDeviceRead<E>,
    offset: usize,
) -> Result<Option<DirectoryEntryNode>, util::Error<E>> {
    let mut dirent_buf = [0; 0xe];
    dev.read(offset, &mut dirent_buf)
        .map_err(|e| util::Error::IOError(e))?;

    // Empty directory entries are filled with 0xff
    if dirent_buf == [0xff; 0xe] {
        return Ok(None);
    }

    let node = DirectoryEntryDiskNode::deserialize(&dirent_buf)?;

    let mut dirent = DirectoryEntryNode {
        node,
        name: [0; 256],
    };

    let name_len = dirent.node.dirent.filename_length as usize;
    let name_buf = &mut dirent.name[0..name_len];
    dev.read(offset + 0xe, name_buf)
        .map_err(|e| util::Error::IOError(e))?;

    Ok(Some(dirent))
}

impl VolumeDescriptor {
    pub fn root_dirent<E>(
        &self,
        dev: &mut impl BlockDeviceRead<E>,
    ) -> Result<Option<DirectoryEntryNode>, util::Error<E>> {
        if self.root_table.is_empty() {
            return Err(util::Error::DirectoryEmpty);
        }

        read_dirent(dev, self.root_table.offset(0)?)
    }
}

impl DirectoryEntryTable {
    fn find_dirent<E>(
        &self,
        dev: &mut impl BlockDeviceRead<E>,
        name: &str,
    ) -> Result<DirectoryEntryNode, util::Error<E>> {
        let mut offset = self.offset(0)?;

        loop {
            let dirent = read_dirent(dev, offset)?;
            let dirent = dirent.ok_or(util::Error::DoesNotExist)?;
            dprintln!(
                "[find_dirent] Found {}: {:?}",
                dirent.get_name(),
                dirent.node
            );
            let dirent_name =
                core::str::from_utf8(dirent.name_slice()).map_err(|e| util::Error::UTFError(e))?;
            dprintln!("[find_dirent] Parsed name: {}", dirent_name);

            let cmp = util::cmp_ignore_case_utf8(name, dirent_name);

            let next_offset = match cmp {
                core::cmp::Ordering::Equal => return Ok(dirent),
                core::cmp::Ordering::Less => dirent.node.left_entry_offset,
                core::cmp::Ordering::Greater => dirent.node.right_entry_offset,
            };

            if next_offset == 0 {
                return Err(util::Error::DoesNotExist);
            }

            offset = self.offset(4 * next_offset as u32)?;
        }
    }

    /// Retrieves the directory entry node corresponding to the provided path,
    /// if it exists.
    ///
    /// Returns None if the root path is provided (root has no dirent)
    /// or the path does not exist.
    pub fn walk_path<E>(
        &self,
        dev: &mut impl BlockDeviceRead<E>,
        path: &str,
    ) -> Result<DirectoryEntryNode, util::Error<E>> {
        if path.is_empty() || path == "/" {
            return Err(util::Error::NoDirent);
        }

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
                return Ok(dirent);
            }

            dirent_tab = dirent_data
                .dirent_table()
                .ok_or(util::Error::DoesNotExist)?;
        }

        Err(util::Error::DoesNotExist)
    }

    /// Walks the directory entry table in preorder, returning all directory entries.
    #[cfg(feature = "alloc")]
    pub fn walk_dirent_tree<E>(
        &self,
        dev: &mut impl BlockDeviceRead<E>,
    ) -> Result<alloc::vec::Vec<DirectoryEntryNode>, util::Error<E>> {
        use alloc::vec;

        let mut dirents = vec![];
        if self.is_empty() {
            return Ok(dirents);
        }

        let mut stack = vec![0];
        while let Some(top) = stack.pop() {
            let offset = self.offset(top)?;
            let dirent = read_dirent(dev, offset)?;

            if let Some(dirent) = dirent {
                dprintln!(
                    "Found dirent {}: {:?} at offset {}",
                    dirent.get_name(),
                    dirent,
                    top
                );

                let left_child = dirent.node.left_entry_offset;
                if left_child != 0 && left_child != 0xffff {
                    stack.push(4 * dirent.node.left_entry_offset as u32);
                }

                let right_child = dirent.node.right_entry_offset;
                if right_child != 0 && right_child != 0xffff {
                    stack.push(4 * dirent.node.right_entry_offset as u32);
                }

                dirents.push(dirent);
            }
        }

        Ok(dirents)
    }

    #[cfg(feature = "alloc")]
    pub fn file_tree<E>(
        &self,
        dev: &mut impl BlockDeviceRead<E>,
    ) -> Result<alloc::vec::Vec<(alloc::string::String, DirectoryEntryNode)>, util::Error<E>> {
        use alloc::format;
        use alloc::string::String;
        use alloc::vec;

        let mut dirents = vec![];

        let mut stack = vec![(String::from(""), *self)];
        while let Some((parent, tree)) = stack.pop() {
            dprintln!("Descending through {}", parent);
            let children = tree.walk_dirent_tree(dev)?;
            for child in children.iter() {
                if let Some(dirent_table) = child.node.dirent.dirent_table() {
                    let child_name = core::str::from_utf8(child.name_slice())
                        .map_err(|e| util::Error::UTFError(e))?;
                    stack.push((format!("{}/{}", parent, child_name), dirent_table));
                }

                dirents.push((parent.clone(), *child));
            }
        }

        Ok(dirents)
    }
}
