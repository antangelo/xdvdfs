use std::ffi::OsStr;

use async_trait::async_trait;
use ciso::{split::SplitFilesystem, write::AsyncWriter};
use xdvdfs::blockdev::BlockDeviceWrite;

use super::{FSWriteWrapper, FileSystemDirectoryHandle};

pub struct CisoOutputDirectory {
    dir: FileSystemDirectoryHandle,
}

impl CisoOutputDirectory {
    pub fn new(dir: FileSystemDirectoryHandle) -> Self {
        Self { dir }
    }
}

#[async_trait]
impl SplitFilesystem<String, FSWriteWrapper> for CisoOutputDirectory {
    async fn create_file(&mut self, name: &OsStr) -> Result<FSWriteWrapper, String> {
        let name = name.to_str().ok_or("Failed to convert name to string")?;
        let file = self
            .dir
            .create_file(name.to_owned())
            .await
            .map_err(|_| "Failed to create file")?;
        let file = FSWriteWrapper::new(&file).await;
        Ok(file)
    }

    async fn close(&mut self, f: FSWriteWrapper) {
        f.close().await;
    }
}

#[async_trait]
impl AsyncWriter for FSWriteWrapper {
    type WriteError = String;

    async fn atomic_write(&mut self, position: u64, data: &[u8]) -> Result<(), String> {
        BlockDeviceWrite::write(self, position, data).await
    }
}
