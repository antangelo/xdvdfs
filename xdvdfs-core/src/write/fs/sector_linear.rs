use crate::{blockdev::BlockDeviceWrite, layout};
use alloc::boxed::Box;
use alloc::vec::Vec;
use maybe_async::maybe_async;

use super::{FileEntry, Filesystem, PathVec};

#[derive(Clone, Debug)]
pub enum SectorLinearBlockContents {
    RawData(Box<[u8; layout::SECTOR_SIZE as usize]>),
    File(PathVec, u64),
    Empty,
}

#[derive(Clone, Debug)]
pub struct SectorLinearBlockDevice<E> {
    contents: alloc::collections::BTreeMap<u64, SectorLinearBlockContents>,

    err_t: core::marker::PhantomData<E>,
}

pub struct SectorLinearBlockFilesystem<'a, E, W: BlockDeviceWrite<E>, F: Filesystem<W, E>> {
    fs: &'a mut F,

    err_t: core::marker::PhantomData<E>,
    bdev_t: core::marker::PhantomData<W>,
}

impl<'a, E, W: BlockDeviceWrite<E>, F: Filesystem<W, E>> SectorLinearBlockFilesystem<'a, E, W, F> {
    pub fn new(fs: &'a mut F) -> Self {
        Self {
            fs,

            err_t: core::marker::PhantomData,
            bdev_t: core::marker::PhantomData,
        }
    }
}

#[maybe_async]
impl<E: Send + Sync> BlockDeviceWrite<E> for SectorLinearBlockDevice<E> {
    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), E> {
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

    async fn len(&mut self) -> Result<u64, E> {
        Ok(self
            .contents
            .last_key_value()
            .map(|(sector, contents)| {
                *sector * layout::SECTOR_SIZE as u64
                    + match contents {
                        SectorLinearBlockContents::RawData(_) => layout::SECTOR_SIZE,
                        SectorLinearBlockContents::File(_, _) => layout::SECTOR_SIZE,
                        SectorLinearBlockContents::Empty => 0,
                    } as u64
            })
            .unwrap_or(0))
    }
}

#[maybe_async]
impl<E, W, F> Filesystem<SectorLinearBlockDevice<E>, E> for SectorLinearBlockFilesystem<'_, E, W, F>
where
    W: BlockDeviceWrite<E>,
    F: Filesystem<W, E>,
    E: Send + Sync,
{
    async fn read_dir(&mut self, path: &PathVec) -> Result<Vec<FileEntry>, E> {
        self.fs.read_dir(path).await
    }

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut SectorLinearBlockDevice<E>,
        offset: u64,
        size: u64,
    ) -> Result<u64, E> {
        let sector = offset / layout::SECTOR_SIZE as u64;
        let offset = offset % layout::SECTOR_SIZE as u64;
        assert_eq!(offset, 0);

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

    async fn copy_file_buf(
        &mut self,
        _src: &PathVec,
        _buf: &mut [u8],
        _offset: u64,
    ) -> Result<u64, E> {
        unimplemented!();
    }
}

impl<E> SectorLinearBlockDevice<E> {
    pub fn num_sectors(&self) -> usize {
        self.contents.len()
    }
}

impl<E> Default for SectorLinearBlockDevice<E> {
    fn default() -> Self {
        Self {
            contents: alloc::collections::BTreeMap::new(),
            err_t: core::marker::PhantomData,
        }
    }
}

impl<E> core::ops::Index<u64> for SectorLinearBlockDevice<E> {
    type Output = SectorLinearBlockContents;

    fn index(&self, index: u64) -> &Self::Output {
        self.contents
            .get(&index)
            .unwrap_or(&SectorLinearBlockContents::Empty)
    }
}

#[cfg(feature = "ciso_support")]
pub struct CisoSectorInput<'a, E: Send + Sync, W: BlockDeviceWrite<E>, F: Filesystem<W, E>> {
    linear: SectorLinearBlockDevice<E>,
    fs: SectorLinearBlockFilesystem<'a, E, W, F>,

    bdev_t: core::marker::PhantomData<W>,
}

#[cfg(feature = "ciso_support")]
impl<'a, E, W, F> CisoSectorInput<'a, E, W, F>
where
    W: BlockDeviceWrite<E>,
    F: Filesystem<W, E>,
    E: Send + Sync,
{
    pub fn new(
        bdev: SectorLinearBlockDevice<E>,
        fs: SectorLinearBlockFilesystem<'a, E, W, F>,
    ) -> Self {
        Self {
            linear: bdev,
            fs,
            bdev_t: core::marker::PhantomData,
        }
    }
}

#[cfg(feature = "ciso_support")]
#[maybe_async]
impl<E, W, F> ciso::write::SectorReader<E> for CisoSectorInput<'_, E, W, F>
where
    W: BlockDeviceWrite<E>,
    F: Filesystem<W, E>,
    E: Send + Sync,
{
    async fn size(&mut self) -> Result<u64, E> {
        self.linear.len().await
    }

    async fn read_sector(&mut self, sector: usize, sector_size: u32) -> Result<Vec<u8>, E> {
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
                    .copy_file_buf(path, &mut buf, sector_size as u64 * sector_idx)
                    .await?;
                assert_eq!(bytes_read, sector_size as u64);
            }
        };

        Ok(buf)
    }
}
