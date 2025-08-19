use std::ffi::OsStr;

use async_trait::async_trait;
use ciso::{split::SplitFilesystem, write::AsyncWriter};
use thiserror::Error;
use xdvdfs::blockdev::BlockDeviceWrite;

use super::{FSWriteError, FSWriteWrapper, FileSystemDirectoryHandle, JsError};

pub struct CisoOutputDirectory {
    dir: FileSystemDirectoryHandle,
}

impl CisoOutputDirectory {
    pub fn new(dir: FileSystemDirectoryHandle) -> Self {
        Self { dir }
    }
}

#[derive(Error, Debug)]
pub enum CisoError {
    #[error("failed to write to block device")]
    Write(#[from] FSWriteError),
    #[error("failed to convert name to UTF-8 string")]
    NameConversion,
    #[error("failed to create file with name {0}")]
    CreateFile(String, #[source] JsError),
}

#[async_trait]
impl SplitFilesystem<CisoError, FSWriteWrapper> for CisoOutputDirectory {
    async fn create_file(&mut self, name: &OsStr) -> Result<FSWriteWrapper, CisoError> {
        let name = name.to_str().ok_or(CisoError::NameConversion)?.to_owned();
        let file = self
            .dir
            .create_file(name.clone())
            .await
            .map_err(JsError::from)
            .map_err(|e| CisoError::CreateFile(name, e))?;
        let file = FSWriteWrapper::new(&file).await;
        Ok(file)
    }

    async fn close(&mut self, f: FSWriteWrapper) {
        f.close().await;
    }
}

#[async_trait]
impl AsyncWriter for FSWriteWrapper {
    type WriteError = CisoError;

    async fn atomic_write(&mut self, position: u64, data: &[u8]) -> Result<(), CisoError> {
        BlockDeviceWrite::write(self, position, data).await?;
        Ok(())
    }
}
