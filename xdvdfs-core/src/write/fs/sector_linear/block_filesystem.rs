use core::convert::Infallible;

use crate::layout;
use crate::write::fs::{
    FileEntry, FilesystemCopier, FilesystemHierarchy, PathVec, SectorLinearBlockContents,
    SectorLinearBlockDevice,
};
use alloc::vec::Vec;
use maybe_async::maybe_async;

#[cfg(not(feature = "sync"))]
use alloc::boxed::Box;

pub struct SectorLinearBlockFilesystem<F> {
    pub fs: F,
}

impl<F> SectorLinearBlockFilesystem<F>
where
    F: FilesystemHierarchy + FilesystemCopier<[u8]>,
{
    pub fn new(fs: F) -> Self {
        Self { fs }
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

#[cfg(test)]
mod test {
    use crate::write::fs::{FilesystemCopier, MemoryFilesystem, PathRef, PathVec};

    use super::{SectorLinearBlockContents, SectorLinearBlockDevice, SectorLinearBlockFilesystem};

    #[test]
    fn test_sector_linear_fs_copier() {
        let memfs = MemoryFilesystem::default();
        let mut slbfs = SectorLinearBlockFilesystem::new(memfs);
        let mut slbd = SectorLinearBlockDevice::default();

        let path: PathRef = "/a/b".into();
        let path: PathVec = path.into();

        futures::executor::block_on(async {
            assert_eq!(
                slbfs.copy_file_in(&path, &mut slbd, 0, 2048, 5000,).await,
                Ok(5000)
            );
        });

        assert_eq!(slbd[0], SectorLinearBlockContents::Empty);
        assert_eq!(slbd[1], SectorLinearBlockContents::File(path.clone(), 0));
        assert_eq!(slbd[2], SectorLinearBlockContents::File(path.clone(), 1));
        assert_eq!(slbd[3], SectorLinearBlockContents::File(path.clone(), 2));
        assert_eq!(slbd[4], SectorLinearBlockContents::Empty);
    }
}
