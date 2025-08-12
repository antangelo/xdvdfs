use maybe_async::maybe_async;

use crate::layout::DirectoryEntryDiskData;
use crate::util;

impl DirectoryEntryDiskData {
    #[maybe_async]
    pub async fn read_data<BDR: crate::blockdev::BlockDeviceRead + ?Sized>(
        &self,
        dev: &mut BDR,
        buf: &mut [u8],
    ) -> Result<(), util::Error<BDR::ReadError>> {
        use crate::util;

        if self.data.size == 0 {
            return Ok(());
        }

        let offset = self.data.offset(0)?;
        dev.read(offset, buf).await.map_err(util::Error::IOError)?;
        Ok(())
    }

    #[maybe_async]
    pub async fn read_data_all<BDR: crate::blockdev::BlockDeviceRead + ?Sized>(
        &self,
        dev: &mut BDR,
    ) -> Result<alloc::boxed::Box<[u8]>, util::Error<BDR::ReadError>> {
        self.read_data_offset(dev, self.data.size as u64, 0).await
    }

    #[maybe_async]
    pub async fn read_data_offset<BDR: crate::blockdev::BlockDeviceRead + ?Sized>(
        &self,
        dev: &mut BDR,
        size: u64,
        offset: u64,
    ) -> Result<alloc::boxed::Box<[u8]>, util::Error<BDR::ReadError>> {
        let size = core::cmp::min(size, self.data.size as u64);
        let buf = alloc::vec![0; size as usize];
        let mut buf = buf.into_boxed_slice();

        if self.data.size == 0 {
            return Ok(buf);
        }

        let offset = self.data.offset(offset)?;
        dev.read(offset, &mut buf)
            .await
            .map_err(util::Error::IOError)?;

        Ok(buf)
    }
}

#[cfg(test)]
mod test {
    use crate::layout::{DirectoryEntryDiskData, DirentAttributes, DiskRegion};
    use futures::executor;

    #[test]
    fn test_layout_dirent_disk_data_read_data_empty() {
        let mut data: [u8; 8] = [1; 8];

        let dirent = DirectoryEntryDiskData {
            data: DiskRegion { sector: 0, size: 0 },
            attributes: DirentAttributes(0),
            filename_length: 0,
        };

        let mut buf = [0; 8];
        executor::block_on(dirent.read_data(data.as_mut_slice(), &mut buf)).unwrap();

        assert_eq!(buf, [0; 8]);
    }

    #[test]
    fn test_layout_dirent_disk_data_read_data_non_empty() {
        let mut data: [u8; 10] = [1; 10];

        let dirent = DirectoryEntryDiskData {
            data: DiskRegion {
                sector: 0,
                size: 10,
            },
            attributes: DirentAttributes(0),
            filename_length: 0,
        };

        let mut buf = [0; 8];
        executor::block_on(dirent.read_data(data.as_mut_slice(), &mut buf)).unwrap();

        assert_eq!(buf, [1; 8]);
    }

    #[test]
    fn test_layout_dirent_disk_data_read_data_all_empty() {
        let mut data: [u8; 8] = [1; 8];

        let dirent = DirectoryEntryDiskData {
            data: DiskRegion { sector: 0, size: 0 },
            attributes: DirentAttributes(0),
            filename_length: 0,
        };

        let data = executor::block_on(dirent.read_data_all(data.as_mut_slice())).unwrap();
        assert_eq!(data.len(), 0);
    }

    #[test]
    fn test_layout_dirent_disk_data_read_data_all_non_empty() {
        let mut data: [u8; 8] = [1; 8];

        let dirent = DirectoryEntryDiskData {
            data: DiskRegion { sector: 0, size: 8 },
            attributes: DirentAttributes(0),
            filename_length: 0,
        };

        let data = executor::block_on(dirent.read_data_all(data.as_mut_slice())).unwrap();
        assert_eq!(data.as_ref(), &[1; 8]);
    }

    #[test]
    fn test_layout_dirent_disk_data_read_data_offset_empty() {
        let mut data: [u8; 8] = [1; 8];

        let dirent = DirectoryEntryDiskData {
            data: DiskRegion { sector: 0, size: 0 },
            attributes: DirentAttributes(0),
            filename_length: 0,
        };

        let data = executor::block_on(dirent.read_data_offset(data.as_mut_slice(), 4, 2))
            .expect("Data should be read without error");
        assert_eq!(data.len(), 0);
    }

    #[test]
    fn test_layout_dirent_disk_data_read_data_offset_non_empty() {
        let mut data: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

        let dirent = DirectoryEntryDiskData {
            data: DiskRegion { sector: 0, size: 8 },
            attributes: DirentAttributes(0),
            filename_length: 0,
        };

        let data = executor::block_on(dirent.read_data_offset(data.as_mut_slice(), 4, 2))
            .expect("Data should be read without error");
        assert_eq!(data.as_ref(), &[3, 4, 5, 6]);
    }
}
