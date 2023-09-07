use std::ffi::OsStr;

use async_trait::async_trait;
use ciso::{
    split::SplitFilesystem,
    write::{AsyncWriter, SectorReader},
};
use xdvdfs::blockdev::BlockDeviceWrite;

use super::{FSWriteWrapper, FileSystemDirectoryHandle, FileSystemFileHandle};

pub struct CisoOutputDirectory {
    dir: FileSystemDirectoryHandle,
}

impl CisoOutputDirectory {
    pub fn new(dir: FileSystemDirectoryHandle) -> Self {
        Self { dir }
    }
}

#[async_trait(?Send)]
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

#[async_trait(?Send)]
impl AsyncWriter<String> for FSWriteWrapper {
    async fn atomic_write(&mut self, position: u64, data: &[u8]) -> Result<(), String> {
        BlockDeviceWrite::write(self, position, data).await
    }
}

#[async_trait(?Send)]
impl SectorReader<String> for FileSystemFileHandle {
    async fn size(&mut self) -> Result<u64, String> {
        let file = self.to_file().await?;
        Ok(file.size() as u64)
    }

    async fn read_sector(&mut self, sector: usize, sector_size: u32) -> Result<Vec<u8>, String> {
        let file = self.to_file().await?;
        let offset: f64 = (sector as f64) * (sector_size as f64);
        let mut buf: Vec<u8> = vec![0; sector_size as usize];

        let slice = file
            .slice_with_f64_and_f64_and_content_type(
                offset,
                offset + sector_size as f64,
                "application/octet-stream",
            )
            .map_err(|_| "failed to slice")?;
        let slice_buf = wasm_bindgen_futures::JsFuture::from(slice.array_buffer())
            .await
            .map_err(|_| "failed to obtain array buffer")?;
        let slice_buf = js_sys::Uint8Array::new(&slice_buf);
        slice_buf.copy_to(&mut buf);

        Ok(buf)
    }
}
