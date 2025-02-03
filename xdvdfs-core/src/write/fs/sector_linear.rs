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

pub struct SectorLinearBlockFilesystem<'a, F: ?Sized> {
    fs: &'a mut F,
}

impl<'a, F> SectorLinearBlockFilesystem<'a, F>
where
    F: FilesystemHierarchy + FilesystemCopier<[u8]> + ?Sized,
{
    pub fn new(fs: &'a mut F) -> Self {
        Self { fs }
    }
}

impl SectorLinearBlockDevice {
    fn len_impl(&self) -> u64 {
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
        Ok(self.len_impl())
    }
}

#[maybe_async]
impl<F> FilesystemHierarchy for SectorLinearBlockFilesystem<'_, F>
where
    F: FilesystemHierarchy,
{
    type Error = <F as FilesystemHierarchy>::Error;

    async fn read_dir(&mut self, path: &PathVec) -> Result<Vec<FileEntry>, Self::Error> {
        self.fs.read_dir(path).await
    }
}

#[maybe_async]
impl<F> FilesystemCopier<SectorLinearBlockDevice> for SectorLinearBlockFilesystem<'_, F>
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

#[cfg(feature = "ciso_support")]
pub struct CisoSectorInput<'a, F> {
    linear: SectorLinearBlockDevice,
    fs: SectorLinearBlockFilesystem<'a, F>,
}

#[cfg(feature = "ciso_support")]
impl<'a, F> CisoSectorInput<'a, F> {
    pub fn new(bdev: SectorLinearBlockDevice, fs: SectorLinearBlockFilesystem<'a, F>) -> Self {
        Self { linear: bdev, fs }
    }
}

#[cfg(feature = "ciso_support")]
#[maybe_async]
impl<F, FSE> ciso::write::SectorReader for CisoSectorInput<'_, F>
where
    F: FilesystemCopier<[u8], Error = FSE>,
{
    type ReadError = FSE;

    async fn size(&mut self) -> Result<u64, FSE> {
        Ok(self.linear.len_impl())
    }

    async fn read_sector(&mut self, sector: usize, sector_size: u32) -> Result<Vec<u8>, FSE> {
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
