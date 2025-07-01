use core::convert::Infallible;

use crate::{blockdev::BlockDeviceWrite, layout, write::fs::PathVec};
use alloc::boxed::Box;
use maybe_async::maybe_async;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SectorLinearBlockContents {
    RawData(Box<[u8; layout::SECTOR_SIZE as usize]>),
    File(PathVec, u64),
    Empty,
}

#[derive(Clone, Debug, Default)]
pub struct SectorLinearBlockDevice {
    pub(super) contents: alloc::collections::BTreeMap<u64, SectorLinearBlockContents>,
}

pub(super) fn new_sector_array() -> Box<[u8; layout::SECTOR_SIZE as usize]> {
    let sector_buf = alloc::vec![0; layout::SECTOR_SIZE as usize];
    let sector_buf: Box<[u8]> = sector_buf.into_boxed_slice();

    // SAFETY: sector_buf always has exactly 2048 elements
    let sector_buf: Box<[u8; layout::SECTOR_SIZE as usize]> = unsafe {
        Box::from_raw(Box::into_raw(sector_buf) as *mut [u8; layout::SECTOR_SIZE as usize])
    };

    sector_buf
}

impl SectorLinearBlockDevice {
    pub fn num_sectors(&self) -> usize {
        self.contents
            .last_key_value()
            .map(|(sector, _)| 1 + *sector as usize)
            .unwrap_or(0)
    }

    pub fn size(&self) -> u64 {
        (self.num_sectors() as u64) * (layout::SECTOR_SIZE as u64)
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

            let mut sector_buf = new_sector_array();
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

#[cfg(test)]
mod test {
    use crate::write::fs::PathVec;

    use super::new_sector_array;

    use super::{SectorLinearBlockContents, SectorLinearBlockDevice};

    #[test]
    fn test_sector_linear_dev_size_end_empty() {
        let mut slbd = SectorLinearBlockDevice::default();
        slbd.contents.insert(5, SectorLinearBlockContents::Empty);
        assert_eq!(slbd.size(), 6 * 2048);
    }

    #[test]
    fn test_sector_linear_dev_size_end_raw_data() {
        let mut slbd = SectorLinearBlockDevice::default();
        let data = new_sector_array();
        slbd.contents
            .insert(5, SectorLinearBlockContents::RawData(data));
        assert_eq!(slbd.size(), 6 * 2048);
    }

    #[test]
    fn test_sector_linear_dev_size_end_file() {
        let mut slbd = SectorLinearBlockDevice::default();
        slbd.contents
            .insert(5, SectorLinearBlockContents::File(PathVec::default(), 0));
        assert_eq!(slbd.size(), 6 * 2048);
    }

    #[test]
    fn test_sector_linear_dev_num_sectors() {
        let mut slbd = SectorLinearBlockDevice::default();
        slbd.contents.insert(5, SectorLinearBlockContents::Empty);
        assert_eq!(slbd.num_sectors(), 6);
    }

    #[test]
    fn test_sector_linear_dev_index_empty() {
        let slbd = SectorLinearBlockDevice::default();
        assert_eq!(slbd[0], SectorLinearBlockContents::Empty);
    }

    #[test]
    fn test_sector_linear_dev_write_aligned() {
        use crate::blockdev::BlockDeviceWrite;

        let mut slbd = SectorLinearBlockDevice::default();
        futures::executor::block_on(async {
            slbd.write(2048, &[1, 2, 3, 4, 5])
                .await
                .expect("write should succeed");
        });

        assert_eq!(slbd[0], SectorLinearBlockContents::Empty);
        let SectorLinearBlockContents::RawData(data) = &slbd[1] else {
            panic!("Sector 1 should contain raw data");
        };
        assert_eq!(data[0..5], [1, 2, 3, 4, 5]);
        assert_eq!(slbd[2], SectorLinearBlockContents::Empty);
    }

    #[test]
    fn test_sector_linear_dev_write_multi_sector() {
        use crate::blockdev::BlockDeviceWrite;

        let mut slbd = SectorLinearBlockDevice::default();
        let data = alloc::vec![10; 2048 * 2];
        futures::executor::block_on(async {
            slbd.write(2048, &data).await.expect("write should succeed");
        });

        assert_eq!(slbd[0], SectorLinearBlockContents::Empty);
        let SectorLinearBlockContents::RawData(data) = &slbd[1] else {
            panic!("Sector 1 should contain raw data");
        };
        assert_eq!(data[0..5], [10, 10, 10, 10, 10]);

        let SectorLinearBlockContents::RawData(data) = &slbd[2] else {
            panic!("Sector 2 should contain raw data");
        };
        assert_eq!(data[0..5], [10, 10, 10, 10, 10]);
        assert_eq!(slbd[3], SectorLinearBlockContents::Empty);
    }
}
