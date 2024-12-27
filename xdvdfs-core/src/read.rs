use super::blockdev::BlockDeviceRead;
use super::layout::{
    self, DirectoryEntryDiskNode, DirectoryEntryNode, DirectoryEntryTable, VolumeDescriptor,
};
use super::util;
use maybe_async::maybe_async;

pub struct DirentScanIter<'a, E, BDR: BlockDeviceRead<E>> {
    sector: usize,
    sector_buf: [u8; layout::SECTOR_SIZE as usize],
    offset: usize,
    end_sector: usize,
    dev: &'a mut BDR,
    err_type: core::marker::PhantomData<E>,
}

impl<E, BDR: BlockDeviceRead<E>> DirentScanIter<'_, E, BDR> {
    #[maybe_async]
    async fn next_sector(&mut self) -> Result<(), util::Error<E>> {
        self.offset = 0;
        self.sector += 1;

        if self.sector >= self.end_sector {
            // Don't bother reading sectors in that we don't care about
            return Ok(());
        }

        self.dev
            .read(
                (self.sector as u64) * (layout::SECTOR_SIZE as u64),
                &mut self.sector_buf,
            )
            .await?;

        Ok(())
    }

    #[maybe_async]
    pub async fn next(&mut self) -> Result<Option<DirectoryEntryNode>, util::Error<E>> {
        if self.sector >= self.end_sector {
            return Ok(None);
        }

        loop {
            // Invariant: offset must remain in bounds of sector_buf
            assert!(self.offset + 0xe < layout::SECTOR_SIZE as usize);

            let mut buf = [0; 0xe];
            let name_offset = self.offset + 0xe;
            buf.copy_from_slice(&self.sector_buf[self.offset..name_offset]);
            let dirent = deserialize_dirent_node(&buf, self.offset as u64)?;
            let Some(mut dirent) = dirent else {
                // If we find an empty record, but we still have sectors to go,
                // advance the sector count and retry
                if self.sector + 1 < self.end_sector {
                    self.next_sector().await?;
                    continue;
                }

                break Ok(None);
            };

            let name_len = dirent.node.dirent.filename_length as usize;
            let name_buf = &mut dirent.name[0..name_len];
            assert!(name_offset + name_len <= layout::SECTOR_SIZE as usize);
            name_buf.copy_from_slice(&self.sector_buf[name_offset..(name_offset + name_len)]);

            // Dirent is valid, advance cursor before returning
            self.offset = name_offset + name_len;
            self.offset += (4 - (self.offset % 4)) % 4;

            if self.offset + 0xe >= layout::SECTOR_SIZE as usize {
                self.next_sector().await?;
            }

            break Ok(Some(dirent));
        }
    }
}

/// Read the XDVDFS volume descriptor from sector 32 of the drive
/// Returns None if the volume descriptor is invalid
#[maybe_async]
pub async fn read_volume<E>(
    dev: &mut impl BlockDeviceRead<E>,
) -> Result<VolumeDescriptor, util::Error<E>> {
    let mut buffer = [0; core::mem::size_of::<VolumeDescriptor>()];
    dev.read(layout::SECTOR_SIZE as u64 * 32, &mut buffer)
        .await
        .map_err(|_| util::Error::InvalidVolume)?;

    let volume = VolumeDescriptor::deserialize(&buffer)?;
    if volume.is_valid() {
        Ok(volume)
    } else {
        Err(util::Error::InvalidVolume)
    }
}

// Deserializes a DirectoryEntryNode from a buffer
// containing a DirectoryEntryDiskNode. This does not
// populate the dirent name.
fn deserialize_dirent_node<E>(
    dirent_buf: &[u8; 0xe],
    offset: u64,
) -> Result<Option<DirectoryEntryNode>, util::Error<E>> {
    // Empty directory entries are filled with 0xff or 0x00
    if dirent_buf == &[0xff; 0xe] || dirent_buf == &[0x00; 0xe] {
        return Ok(None);
    }

    let node = DirectoryEntryDiskNode::deserialize(dirent_buf)?;
    Ok(Some(DirectoryEntryNode {
        node,
        name: [0; 256],
        offset,
    }))
}

#[maybe_async]
async fn read_dirent<E>(
    dev: &mut impl BlockDeviceRead<E>,
    offset: u64,
) -> Result<Option<DirectoryEntryNode>, util::Error<E>> {
    let mut dirent_buf = [0; 0xe];
    dev.read(offset, &mut dirent_buf)
        .await
        .map_err(|e| util::Error::IOError(e))?;

    let dirent = deserialize_dirent_node(&dirent_buf, offset)?;
    if let Some(mut dirent) = dirent {
        let name_len = dirent.node.dirent.filename_length as usize;
        let name_buf = &mut dirent.name[0..name_len];
        dev.read(offset + 0xe, name_buf)
            .await
            .map_err(|e| util::Error::IOError(e))?;

        Ok(Some(dirent))
    } else {
        Ok(None)
    }
}

impl VolumeDescriptor {
    #[maybe_async]
    pub async fn root_dirent<E>(
        &self,
        dev: &mut impl BlockDeviceRead<E>,
    ) -> Result<Option<DirectoryEntryNode>, util::Error<E>> {
        if self.root_table.is_empty() {
            return Err(util::Error::DirectoryEmpty);
        }

        read_dirent(dev, self.root_table.offset(0)?).await
    }
}

impl DirectoryEntryTable {
    #[maybe_async]
    async fn find_dirent<E>(
        &self,
        dev: &mut impl BlockDeviceRead<E>,
        name: &str,
    ) -> Result<DirectoryEntryNode, util::Error<E>> {
        debugln!("[find_dirent] Called on {}", name);
        if self.is_empty() {
            return Err(util::Error::DirectoryEmpty);
        }

        let mut offset = self.offset(0)?;

        loop {
            let dirent = read_dirent(dev, offset).await?;
            let dirent = dirent.ok_or(util::Error::DoesNotExist)?;
            let dirent_name = dirent.name_str()?;
            debugln!("[find_dirent] Found {}", dirent_name);
            traceln!("[find_dirent] Node: {:?}", dirent.node);

            let cmp = util::cmp_ignore_case_utf8(name, &dirent_name);
            debugln!("[find_dirent] Comparison result: {:?}", cmp);

            let next_offset = match cmp {
                core::cmp::Ordering::Equal => return Ok(dirent),
                core::cmp::Ordering::Less => dirent.node.left_entry_offset,
                core::cmp::Ordering::Greater => dirent.node.right_entry_offset,
            };

            if next_offset == 0 {
                return Err(util::Error::DoesNotExist);
            }

            offset = self.offset(4 * next_offset as u64)?;
        }
    }

    /// Retrieves the directory entry node corresponding to the provided path,
    /// if it exists.
    ///
    /// Returns None if the root path is provided (root has no dirent)
    /// or the path does not exist.
    #[maybe_async]
    pub async fn walk_path<E>(
        &self,
        dev: &mut impl BlockDeviceRead<E>,
        path: &str,
    ) -> Result<DirectoryEntryNode, util::Error<E>> {
        debugln!("[walk_path] Called on {}", path);
        if path.is_empty() || path == "/" {
            return Err(util::Error::NoDirent);
        }

        let mut dirent_tab = *self;
        let mut path_iter = path
            .trim_start_matches('/')
            .split_terminator('/')
            .peekable();

        while let Some(segment) = path_iter.next() {
            let dirent = dirent_tab.find_dirent(dev, segment).await?;
            debugln!("[walk_path] Found dirent: {}", dirent.name_str()?);
            traceln!("[walk_path] Node: {:?}", dirent.node);
            let dirent_data = &dirent.node.dirent;

            if path_iter.peek().is_none() {
                return Ok(dirent);
            }

            dirent_tab = dirent_data
                .dirent_table()
                .ok_or(util::Error::IsNotDirectory)?;
        }

        unreachable!("path_iter has been consumed without returning last dirent")
    }

    /// Scan the dirent tree iteratively, reading an entire sector at a time
    /// Order is not guaranteed, but reads are batched
    /// Returns an async iterator that reads sequential records
    #[maybe_async]
    pub async fn scan_dirent_tree<'a, E, BDR: BlockDeviceRead<E>>(
        &self,
        dev: &'a mut BDR,
    ) -> Result<DirentScanIter<'a, E, BDR>, util::Error<E>> {
        if self.is_empty() {
            // Return a dummy scan iterator that is 0xff filled.
            // This is considered to be an empty sector, and avoids
            // needing to read data or maintain other empty state.
            return Ok(DirentScanIter {
                sector: 0,
                sector_buf: [0xff; layout::SECTOR_SIZE as usize],
                offset: 0,
                end_sector: 0,
                dev,
                err_type: core::marker::PhantomData,
            });
        }

        let mut sector_buf = [0; layout::SECTOR_SIZE as usize];
        let sector_offset = self.offset(0)?;
        dev.read(sector_offset, &mut sector_buf).await?;

        let sector = self.region.sector as usize;
        Ok(DirentScanIter {
            sector,
            sector_buf,
            offset: 0,
            end_sector: sector + self.region.size.div_ceil(layout::SECTOR_SIZE) as usize,
            dev,
            err_type: core::marker::PhantomData,
        })
    }

    /// Walks the directory entry table in preorder, returning all directory entries.
    #[maybe_async]
    pub async fn walk_dirent_tree<E>(
        &self,
        dev: &mut impl BlockDeviceRead<E>,
    ) -> Result<alloc::vec::Vec<DirectoryEntryNode>, util::Error<E>> {
        use alloc::vec;

        debugln!("[walk_dirent_tree] {:?}", self);

        let mut dirents = vec![];
        if self.is_empty() {
            return Ok(dirents);
        }

        let mut stack = vec![0];
        while let Some(top) = stack.pop() {
            let offset = self.offset(top)?;
            let dirent = read_dirent(dev, offset).await?;

            if let Some(dirent) = dirent {
                debugln!("[walk_dirent_tree] Found dirent {}", dirent.name_str()?);
                traceln!("[walk_dirent_tree] Node: {:?} at offset {}", dirent, top);

                let left_child = dirent.node.left_entry_offset;
                if left_child != 0 && left_child != 0xffff {
                    stack.push(4 * dirent.node.left_entry_offset as u64);
                }

                let right_child = dirent.node.right_entry_offset;
                if right_child != 0 && right_child != 0xffff {
                    stack.push(4 * dirent.node.right_entry_offset as u64);
                }

                dirents.push(dirent);
            }
        }

        Ok(dirents)
    }

    #[maybe_async]
    pub async fn file_tree<E>(
        &self,
        dev: &mut impl BlockDeviceRead<E>,
    ) -> Result<alloc::vec::Vec<(alloc::string::String, DirectoryEntryNode)>, util::Error<E>> {
        use alloc::format;
        use alloc::string::String;
        use alloc::vec;

        let mut dirents = vec![];

        let mut stack = vec![(String::from(""), *self)];
        while let Some((parent, tree)) = stack.pop() {
            debugln!("[file_tree] Descending through {}", parent);
            let children = tree.walk_dirent_tree(dev).await?;
            for child in children.iter() {
                if let Some(dirent_table) = child.node.dirent.dirent_table() {
                    let child_name = child.name_str()?;
                    stack.push((format!("{}/{}", parent, child_name), dirent_table));
                }

                dirents.push((parent.clone(), *child));
            }
        }

        Ok(dirents)
    }
}
