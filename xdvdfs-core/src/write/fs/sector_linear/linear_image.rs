use crate::layout;
use crate::write::fs::sector_linear::deferred_reader::DeferredFileRead;
use crate::write::fs::{
    FilesystemCopier, SectorLinearBlockContents, SectorLinearBlockDevice,
    SectorLinearBlockFilesystem,
};
use alloc::{vec, vec::Vec};
use maybe_async::maybe_async;

#[cfg(not(feature = "sync"))]
use alloc::boxed::Box;

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

impl<F, FSE> SectorLinearImage<'_, F>
where
    F: FilesystemCopier<[u8], Error = FSE>,
{
    #[maybe_async]
    pub async fn read_linear(&mut self, offset: u64, size: u64) -> Result<Vec<u8>, FSE> {
        let mut sector = offset / (layout::SECTOR_SIZE as u64);
        let mut position = offset % (layout::SECTOR_SIZE as u64);

        let size = size as usize;
        let mut buffer = vec![0; size];
        let mut index: usize = 0;

        let mut iter = self.linear.contents.range(sector..);

        let mut deferred_file_read = DeferredFileRead::default();

        while index < size {
            let Some((incoming_sector, contents)) = iter.next() else {
                // Out of sectors, truncate buffer to actual size
                buffer.resize(index, 0);
                break;
            };

            if *incoming_sector > sector {
                let sector_gap = *incoming_sector - sector;
                let empty_len = core::cmp::min(
                    (size - index) as u64,
                    sector_gap * layout::SECTOR_SIZE as u64 - position,
                );
                index += empty_len as usize;
                position = 0;
                sector += sector_gap;

                if index >= size {
                    break;
                }
            }

            let remaining = size - index;
            let to_read =
                core::cmp::min(remaining as u64, layout::SECTOR_SIZE as u64 - position) as usize;

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
        let offset = sector as u64 * sector_size as u64;
        let mut data = self.read_linear(offset, sector_size as u64).await?;
        if data.len() < sector_size as usize {
            data.resize(sector_size as usize, 0);
        }

        Ok(data)
    }
}

#[cfg(test)]
mod test {
    use crate::write::fs::{sector_linear::new_sector_array, MemoryFilesystem, PathVec};

    use super::{
        SectorLinearBlockContents, SectorLinearBlockDevice, SectorLinearBlockFilesystem,
        SectorLinearImage,
    };

    #[test]
    fn test_linear_image_read_empty_entry() {
        let memfs = MemoryFilesystem::default();
        let mut slbd = SectorLinearBlockDevice::default();
        let mut slbfs = SectorLinearBlockFilesystem::new(memfs);

        slbd.contents.insert(0, SectorLinearBlockContents::Empty);

        let mut image = SectorLinearImage::new(&slbd, &mut slbfs);
        futures::executor::block_on(async {
            let data = image
                .read_linear(0, 2048)
                .await
                .expect("Read should return data");
            assert_eq!(data.len(), 2048);
            assert!(data.iter().all(|x| *x == 0));
        });
    }

    #[test]
    fn test_linear_image_read_empty_image() {
        let memfs = MemoryFilesystem::default();
        let mut slbd = SectorLinearBlockDevice::default();
        let mut slbfs = SectorLinearBlockFilesystem::new(memfs);

        // Insert sector to give image size
        slbd.contents.insert(5, SectorLinearBlockContents::Empty);

        let mut image = SectorLinearImage::new(&slbd, &mut slbfs);
        futures::executor::block_on(async {
            let data = image
                .read_linear(0, 2048)
                .await
                .expect("Read should return data");
            assert_eq!(data.len(), 2048);
            assert!(data.iter().all(|x| *x == 0));
        });
    }

    #[test]
    fn test_linear_image_eof_truncated_output() {
        let memfs = MemoryFilesystem::default();
        let mut slbd = SectorLinearBlockDevice::default();
        let mut slbfs = SectorLinearBlockFilesystem::new(memfs);

        // Insert sector to give image size
        slbd.contents.insert(0, SectorLinearBlockContents::Empty);

        let mut image = SectorLinearImage::new(&slbd, &mut slbfs);
        futures::executor::block_on(async {
            let data = image
                .read_linear(0, 4096)
                .await
                .expect("Read should return data");
            assert_eq!(data.len(), 2048);
            assert!(data.iter().all(|x| *x == 0));
        });
    }

    #[test]
    fn test_linear_image_read_data() {
        let memfs = MemoryFilesystem::default();
        let mut slbd = SectorLinearBlockDevice::default();
        let mut slbfs = SectorLinearBlockFilesystem::new(memfs);

        let mut data = new_sector_array();
        data.fill(10);
        slbd.contents
            .insert(0, SectorLinearBlockContents::RawData(data));

        let mut image = SectorLinearImage::new(&slbd, &mut slbfs);
        futures::executor::block_on(async {
            let data = image
                .read_linear(0, 2048)
                .await
                .expect("Read should return data");
            assert_eq!(data.len(), 2048);
            assert!(data.iter().all(|x| *x == 10));
        });
    }

    #[test]
    fn test_linear_image_read_data_offset() {
        let memfs = MemoryFilesystem::default();
        let mut slbd = SectorLinearBlockDevice::default();
        let mut slbfs = SectorLinearBlockFilesystem::new(memfs);

        let mut data = new_sector_array();
        data.fill(10);
        slbd.contents
            .insert(0, SectorLinearBlockContents::RawData(data));
        slbd.contents.insert(1, SectorLinearBlockContents::Empty);

        let mut image = SectorLinearImage::new(&slbd, &mut slbfs);
        futures::executor::block_on(async {
            let data = image
                .read_linear(1024, 2048)
                .await
                .expect("Read should return data");
            assert_eq!(data.len(), 2048);
            assert!(data[0..1024].iter().all(|x| *x == 10));
            assert!(data[1024..].iter().all(|x| *x == 0));
        });
    }

    #[test]
    fn test_linear_image_read_data_sized() {
        let memfs = MemoryFilesystem::default();
        let mut slbd = SectorLinearBlockDevice::default();
        let mut slbfs = SectorLinearBlockFilesystem::new(memfs);

        let mut data = new_sector_array();
        data.fill(10);
        slbd.contents
            .insert(0, SectorLinearBlockContents::RawData(data));

        let mut image = SectorLinearImage::new(&slbd, &mut slbfs);
        futures::executor::block_on(async {
            let data = image
                .read_linear(0, 1024)
                .await
                .expect("Read should return data");
            assert_eq!(data.len(), 1024);
            assert!(data.iter().all(|x| *x == 10));
        });
    }

    #[test]
    fn test_linear_image_read_file() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &alloc::vec![10; 4000]);

        let mut slbd = SectorLinearBlockDevice::default();
        let mut slbfs = SectorLinearBlockFilesystem::new(memfs);

        let path = PathVec::from("/a/b");
        slbd.contents
            .insert(0, SectorLinearBlockContents::File(path.clone(), 0));
        slbd.contents
            .insert(1, SectorLinearBlockContents::File(path, 1));

        let mut image = SectorLinearImage::new(&slbd, &mut slbfs);
        futures::executor::block_on(async {
            let data = image
                .read_linear(0, 4096)
                .await
                .expect("Read should return data");
            assert_eq!(data.len(), 4096);
            assert!(data[0..4000].iter().all(|x| *x == 10));
            assert!(data[4000..].iter().all(|x| *x == 0));
        });
    }

    #[test]
    fn test_linear_image_read_file_offset() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &alloc::vec![10; 4000]);

        let mut slbd = SectorLinearBlockDevice::default();
        let mut slbfs = SectorLinearBlockFilesystem::new(memfs);

        let path = PathVec::from("/a/b");
        slbd.contents
            .insert(0, SectorLinearBlockContents::File(path.clone(), 0));
        slbd.contents
            .insert(1, SectorLinearBlockContents::File(path, 1));

        let mut image = SectorLinearImage::new(&slbd, &mut slbfs);
        futures::executor::block_on(async {
            let data = image
                .read_linear(1024, 4096)
                .await
                .expect("Read should return data");
            assert_eq!(data.len(), 3072);

            // File size of 4000 - offset of 1024
            assert!(data[0..2976].iter().all(|x| *x == 10));
            assert!(data[2976..].iter().all(|x| *x == 0));
        });
    }

    #[test]
    fn test_linear_image_read_file_sized() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &alloc::vec![10; 4000]);

        let mut slbd = SectorLinearBlockDevice::default();
        let mut slbfs = SectorLinearBlockFilesystem::new(memfs);

        let path = PathVec::from("/a/b");
        slbd.contents
            .insert(0, SectorLinearBlockContents::File(path.clone(), 0));
        slbd.contents
            .insert(1, SectorLinearBlockContents::File(path, 1));

        let mut image = SectorLinearImage::new(&slbd, &mut slbfs);
        futures::executor::block_on(async {
            let data = image
                .read_linear(0, 3072)
                .await
                .expect("Read should return data");
            assert_eq!(data.len(), 3072);
            assert!(data[0..3072].iter().all(|x| *x == 10));
            assert!(data[3072..].iter().all(|x| *x == 0));
        });
    }

    #[test]
    fn test_linear_image_read_sparse_multi_sector() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &alloc::vec![10; 2000]);

        let mut slbd = SectorLinearBlockDevice::default();
        let mut slbfs = SectorLinearBlockFilesystem::new(memfs);

        slbd.contents
            .insert(3, SectorLinearBlockContents::File("/a/b".into(), 0));
        let mut data = new_sector_array();
        data.fill(15);
        slbd.contents
            .insert(5, SectorLinearBlockContents::RawData(data));

        let mut image = SectorLinearImage::new(&slbd, &mut slbfs);
        futures::executor::block_on(async {
            // Size: 1024 from sector 1, then sectors 2, 3, 4, 5
            let data = image
                .read_linear(3072, 2048 * 4 + 1024)
                .await
                .expect("Read should return data");
            assert_eq!(data.len(), 2048 * 4 + 1024);

            // Sector 1 and 2 are empty
            assert_eq!(data[0..3072], alloc::vec![0; 3072]);
            // Sector 3 is a file, last 48 bytes are zero
            assert_eq!(data[3072..5072], alloc::vec![10; 2000]);
            assert_eq!(data[5072..5120], alloc::vec![0; 48]);
            // Sector 4 is empty
            assert_eq!(data[5120..7168], alloc::vec![0; 2048]);
            // Sector 5 is data
            assert_eq!(data[7168..], alloc::vec![15; 2048]);
        })
    }
}
