use core::convert::Infallible;

use crate::{blockdev::BlockDeviceWrite, layout};
use alloc::boxed::Box;
use alloc::vec::Vec;
use maybe_async::maybe_async;

use super::{FileEntry, FilesystemCopier, FilesystemHierarchy, PathVec};

#[derive(Clone, Debug)]
pub enum SectorLinearBlockContents {
    RawData(Box<[u8; layout::SECTOR_SIZE as usize]>),
    File(PathVec, u64),
    Empty,
}

#[derive(Clone, Debug, Default)]
pub struct SectorLinearBlockDevice {
    contents: alloc::collections::BTreeMap<u64, SectorLinearBlockContents>,
}

pub struct SectorLinearBlockFilesystem<F> {
    fs: F,
}

impl<F> SectorLinearBlockFilesystem<F>
where
    F: FilesystemHierarchy + FilesystemCopier<[u8]>,
{
    pub fn new(fs: F) -> Self {
        Self { fs }
    }
}

impl SectorLinearBlockDevice {
    pub fn size(&self) -> u64 {
        self.contents
            .last_key_value()
            .map(|(sector, contents)| {
                *sector * layout::SECTOR_SIZE as u64
                    + match contents {
                        SectorLinearBlockContents::RawData(_) => layout::SECTOR_SIZE,
                        SectorLinearBlockContents::File(_, _) => layout::SECTOR_SIZE,
                        SectorLinearBlockContents::Empty => 0,
                    } as u64
            })
            .unwrap_or(0)
    }
}

#[maybe_async]
impl BlockDeviceWrite for SectorLinearBlockDevice {
    type WriteError = Infallible;

    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), Infallible> {
        let mut remaining = buffer.len();
        let mut buffer_pos = 0;

        let mut sector = offset / layout::SECTOR_SIZE as u64;

        let offset = offset % layout::SECTOR_SIZE as u64;
        assert_eq!(offset, 0);

        while remaining > 0 {
            let to_write = core::cmp::min(layout::SECTOR_SIZE as usize, remaining);

            let sector_buf = alloc::vec![0; layout::SECTOR_SIZE as usize];
            let sector_buf: Box<[u8]> = sector_buf.into_boxed_slice();
            let mut sector_buf: Box<[u8; layout::SECTOR_SIZE as usize]> = unsafe {
                Box::from_raw(Box::into_raw(sector_buf) as *mut [u8; layout::SECTOR_SIZE as usize])
            };

            sector_buf[0..to_write].copy_from_slice(&buffer[buffer_pos..(buffer_pos + to_write)]);

            if self
                .contents
                .insert(sector, SectorLinearBlockContents::RawData(sector_buf))
                .is_some()
            {
                unimplemented!("Overwriting sectors is not implemented");
            }

            remaining -= to_write;
            buffer_pos += to_write;
            sector += 1;
        }

        Ok(())
    }

    async fn len(&mut self) -> Result<u64, Infallible> {
        Ok(self.size())
    }
}

#[maybe_async]
impl<F> FilesystemHierarchy for SectorLinearBlockFilesystem<F>
where
    F: FilesystemHierarchy,
{
    type Error = <F as FilesystemHierarchy>::Error;

    async fn read_dir(&mut self, path: &PathVec) -> Result<Vec<FileEntry>, Self::Error> {
        self.fs.read_dir(path).await
    }
}

#[maybe_async]
impl<F> FilesystemCopier<SectorLinearBlockDevice> for SectorLinearBlockFilesystem<F>
where
    F: FilesystemCopier<[u8]>,
{
    type Error = Infallible;

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut SectorLinearBlockDevice,
        input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, Self::Error> {
        assert_eq!(input_offset, 0);

        let sector = output_offset / layout::SECTOR_SIZE as u64;
        let output_offset = output_offset % layout::SECTOR_SIZE as u64;
        assert_eq!(output_offset, 0);

        let mut sector_span = size / layout::SECTOR_SIZE as u64;
        if size % layout::SECTOR_SIZE as u64 > 0 {
            sector_span += 1;
        }

        for i in 0..sector_span {
            if dest
                .contents
                .insert(sector + i, SectorLinearBlockContents::File(src.clone(), i))
                .is_some()
            {
                unimplemented!("Overwriting sectors is not implemented");
            }
        }

        Ok(size)
    }
}

impl SectorLinearBlockDevice {
    pub fn num_sectors(&self) -> usize {
        self.contents.len()
    }
}

impl core::ops::Index<u64> for SectorLinearBlockDevice {
    type Output = SectorLinearBlockContents;

    fn index(&self, index: u64) -> &Self::Output {
        self.contents
            .get(&index)
            .unwrap_or(&SectorLinearBlockContents::Empty)
    }
}

pub struct SectorLinearImage<'a, F> {
    linear: &'a SectorLinearBlockDevice,
    fs: &'a mut SectorLinearBlockFilesystem<F>,
}

impl<'a, F> SectorLinearImage<'a, F> {
    pub fn new(
        bdev: &'a SectorLinearBlockDevice,
        fs: &'a mut SectorLinearBlockFilesystem<F>,
    ) -> Self {
        Self { linear: bdev, fs }
    }
}

struct DeferredFileReadInner {
    path: PathVec,
    index: usize,
    offset: u64,
    size: u64,
    prev_sector_idx: u64,
}

#[derive(Default)]
struct DeferredFileRead(Option<DeferredFileReadInner>);

impl DeferredFileRead {
    async fn commit<FSE, F: FilesystemCopier<[u8], Error = FSE>>(
        &mut self,
        fs: &mut F,
        buffer: &mut Vec<u8>,
    ) -> Result<(), FSE> {
        let Some(dfr) = &self.0 else {
            return Ok(());
        };

        let limit = dfr.index + dfr.size as usize;
        fs.copy_file_in(
            &dfr.path,
            &mut buffer[dfr.index..limit],
            dfr.offset,
            0,
            dfr.size,
        )
        .await?;

        self.0 = None;
        Ok(())
    }

    async fn push_file<FSE, F: FilesystemCopier<[u8], Error = FSE>>(
        &mut self,
        fs: &mut F,
        buffer: &mut Vec<u8>,
        path: &PathVec,
        sector_idx: u64,
        to_read: u64,
        buffer_idx: usize,
        position: u64,
    ) -> Result<(), FSE> {
        if let Some(dfr) = &mut self.0 {
            // If the path and sector offsets line up, defer the read
            if dfr.path == *path && dfr.prev_sector_idx + 1 == sector_idx {
                dfr.size += to_read;
                dfr.prev_sector_idx = sector_idx;
                return Ok(());
            }

            // Otherwise, we have to push a new file
            self.commit(fs, buffer).await?;
        }

        self.0 = Some(DeferredFileReadInner {
            path: path.clone(),
            index: buffer_idx,
            offset: position + sector_idx * layout::SECTOR_SIZE as u64,
            size: to_read,
            prev_sector_idx: sector_idx,
        });
        Ok(())
    }
}

impl<F, FSE> SectorLinearImage<'_, F>
where
    F: FilesystemCopier<[u8], Error = FSE>,
{
    pub async fn read_linear(&mut self, offset: u64, size: u64) -> Result<Vec<u8>, FSE> {
        let mut sector = offset / (layout::SECTOR_SIZE as u64);
        let mut position = offset % (layout::SECTOR_SIZE as u64);

        // FIXME: Handle out of bounds

        let size = size as usize;
        let mut buffer = Vec::new();
        buffer.resize(size, 0);
        let mut index: usize = 0;

        let mut iter = self.linear.contents.range(sector..).peekable();

        let mut deferred_file_read = DeferredFileRead::default();

        while index < size {
            let remaining = size - index;
            let to_read =
                core::cmp::min(remaining as u64, layout::SECTOR_SIZE as u64 - position) as usize;

            let Some((incoming_sector, _)) = iter.peek() else {
                // Out of sectors, truncate buffer to actual size
                buffer.resize(index, 0);
                break;
            };

            // Handle empty sectors
            if **incoming_sector != sector {
                index += to_read;
                position = 0;
                sector += 1;
                continue;
            }

            let (_, contents) = iter.next().expect("Empty iter handled in peek case");
            match contents {
                SectorLinearBlockContents::Empty => {}
                SectorLinearBlockContents::RawData(data) => {
                    let position = position as usize;
                    let end = position + to_read;
                    buffer[index..(index + to_read)].clone_from_slice(&data[position..end]);
                }
                SectorLinearBlockContents::File(path, sector_idx) => {
                    deferred_file_read
                        .push_file(
                            &mut self.fs.fs,
                            &mut buffer,
                            path,
                            *sector_idx,
                            to_read as u64,
                            index,
                            position,
                        )
                        .await?
                }
            }

            index += to_read;
            position = 0;
            sector += 1;
        }

        deferred_file_read
            .commit(&mut self.fs.fs, &mut buffer)
            .await?;
        Ok(buffer)
    }
}

#[cfg(feature = "ciso_support")]
#[maybe_async]
impl<F, FSE> ciso::write::SectorReader for SectorLinearImage<'_, F>
where
    F: FilesystemCopier<[u8], Error = FSE>,
{
    type ReadError = FSE;

    async fn size(&mut self) -> Result<u64, FSE> {
        Ok(self.linear.size())
    }

    async fn read_sector(&mut self, sector: usize, sector_size: u32) -> Result<Vec<u8>, FSE> {
        // FIXME: Assumes sector_size == layout::SECTOR_SIZE
        assert_eq!(sector_size, layout::SECTOR_SIZE);
        let mut buf = alloc::vec![0; sector_size as usize];

        match &self.linear[sector as u64] {
            SectorLinearBlockContents::Empty => {}
            SectorLinearBlockContents::RawData(data) => {
                buf.copy_from_slice(data.as_slice());
            }
            SectorLinearBlockContents::File(path, sector_idx) => {
                let bytes_read = self
                    .fs
                    .fs
                    .copy_file_in(
                        path,
                        &mut buf,
                        sector_size as u64 * sector_idx,
                        0,
                        sector_size as u64,
                    )
                    .await?;
                assert_eq!(bytes_read, sector_size as u64);
            }
        };

        Ok(buf)
    }
}
