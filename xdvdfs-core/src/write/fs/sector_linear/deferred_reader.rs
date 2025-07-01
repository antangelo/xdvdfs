use maybe_async::maybe_async;

use crate::layout;
use crate::write::fs::{FilesystemCopier, PathRef, PathVec};

struct DeferredFileReadInner {
    path: PathVec,
    index: usize,
    offset: u64,
    size: u64,
    prev_sector_idx: u64,
}

#[derive(Default)]
pub(super) struct DeferredFileRead(Option<DeferredFileReadInner>);

impl DeferredFileRead {
    #[maybe_async]
    pub(super) async fn commit<FSE, F: FilesystemCopier<[u8], Error = FSE>>(
        &mut self,
        fs: &mut F,
        buffer: &mut [u8],
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

    #[maybe_async]
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn push_file<
        'a,
        FSE,
        F: FilesystemCopier<[u8], Error = FSE>,
        P: Into<PathRef<'a>>,
    >(
        &mut self,
        fs: &mut F,
        buffer: &mut [u8],
        path: P,
        sector_idx: u64,
        to_read: u64,
        buffer_idx: usize,
        position: u64,
    ) -> Result<(), FSE> {
        let path: PathRef = path.into();
        if let Some(dfr) = &mut self.0 {
            // If the path and sector offsets line up, defer the read
            if path == (&dfr.path).into() && dfr.prev_sector_idx + 1 == sector_idx {
                dfr.size += to_read;
                dfr.prev_sector_idx = sector_idx;
                return Ok(());
            }

            // Otherwise, we have to push a new file
            self.commit(fs, buffer).await?;
        }

        self.0 = Some(DeferredFileReadInner {
            path: path.into(),
            index: buffer_idx,
            offset: position + sector_idx * layout::SECTOR_SIZE as u64,
            size: to_read,
            prev_sector_idx: sector_idx,
        });
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::DeferredFileRead;
    use crate::write::fs::MemoryFilesystem;

    #[test]
    fn test_deferred_reader_commit_empty() {
        let mut memfs = MemoryFilesystem::default();
        let mut dfr = DeferredFileRead::default();
        let mut buffer = alloc::vec![0; 512];

        futures::executor::block_on(async {
            assert_eq!(dfr.commit(&mut memfs, &mut buffer).await, Ok(()));
        })
    }

    #[test]
    fn test_deferred_reader_push_commit() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &[1, 2, 3, 4, 5]);

        let mut dfr = DeferredFileRead::default();
        let mut buffer = alloc::vec![0; 5];

        futures::executor::block_on(async {
            let result = dfr
                .push_file(&mut memfs, &mut buffer, "/a/b", 0, 3, 1, 1)
                .await;

            // Assert read is deferred
            assert_eq!(result, Ok(()));
            assert_eq!(buffer, [0, 0, 0, 0, 0]);

            assert_eq!(dfr.commit(&mut memfs, &mut buffer).await, Ok(()));
            assert_eq!(buffer, [0, 2, 3, 4, 0]);
        });
    }

    #[test]
    fn test_deferred_reader_multi_sector() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &alloc::vec![10; 4096]);

        let mut dfr = DeferredFileRead::default();
        let mut buffer = alloc::vec![0; 4096];

        futures::executor::block_on(async {
            let result = dfr
                .push_file(
                    &mut memfs,
                    &mut buffer,
                    "/a/b",
                    /*sector_idx=*/ 0,
                    /*to_read=*/ 2048,
                    /*buffer_idx=*/ 0,
                    /*position=*/ 0,
                )
                .await;
            assert_eq!(result, Ok(()));
            assert!(buffer.iter().all(|x| *x == 0));

            let result = dfr
                .push_file(
                    &mut memfs,
                    &mut buffer,
                    "/a/b",
                    /*sector_idx=*/ 1,
                    /*to_read=*/ 2048,
                    /*buffer_idx=*/ 2048,
                    /*position=*/ 0,
                )
                .await;
            assert_eq!(result, Ok(()));
            assert!(buffer.iter().all(|x| *x == 0));

            assert_eq!(dfr.commit(&mut memfs, &mut buffer).await, Ok(()));
            assert!(buffer.iter().all(|x| *x == 10));
        });
    }

    #[test]
    fn test_deferred_reader_switch_file() {
        let mut memfs = MemoryFilesystem::default();
        memfs.create("/a/b", &alloc::vec![10; 2048]);
        memfs.create("/a/c", &alloc::vec![15; 2048]);

        let mut dfr = DeferredFileRead::default();
        let mut buffer = alloc::vec![0; 4096];

        futures::executor::block_on(async {
            let result = dfr
                .push_file(
                    &mut memfs,
                    &mut buffer,
                    "/a/b",
                    /*sector_idx=*/ 0,
                    /*to_read=*/ 2048,
                    /*buffer_idx=*/ 0,
                    /*position=*/ 0,
                )
                .await;
            assert_eq!(result, Ok(()));
            assert!(buffer.iter().all(|x| *x == 0));

            let result = dfr
                .push_file(
                    &mut memfs,
                    &mut buffer,
                    "/a/c",
                    /*sector_idx=*/ 0,
                    /*to_read=*/ 2048,
                    /*buffer_idx=*/ 2048,
                    /*position=*/ 0,
                )
                .await;
            assert_eq!(result, Ok(()));

            // Previous read is applied
            assert!(buffer[0..2048].iter().all(|x| *x == 10));
            assert!(buffer[2048..].iter().all(|x| *x == 0));

            // Current read is applied
            assert_eq!(dfr.commit(&mut memfs, &mut buffer).await, Ok(()));
            assert!(buffer[0..2048].iter().all(|x| *x == 10));
            assert!(buffer[2048..].iter().all(|x| *x == 15));
        });
    }
}
