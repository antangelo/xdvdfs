use anyhow::Context;
use async_recursion::async_recursion;
use async_trait::async_trait;
use std::collections::BTreeMap;
use thiserror::Error;
use xdvdfs::write::fs::{PathRef, PathVec};

pub mod bindings;
pub use bindings::*;
pub mod ciso;

mod util;

// FIXME: Abstract backend enough that this is no longer needed to leak
pub use util::JsError;
use util::UnsafeJSFuture;

pub struct FSWriteWrapper {
    stream: FileSystemWritableFileStream,
    len: u64,
}

unsafe impl Send for FSWriteWrapper {}
unsafe impl Sync for FSWriteWrapper {}

impl FSWriteWrapper {
    pub async fn new(fh: &FileSystemFileHandle) -> Self {
        let stream = fh.writable_stream().await.unwrap();
        Self { stream, len: 0 }
    }

    pub async fn close(self) {
        UnsafeJSFuture::from(self.stream.close()).await.unwrap();
    }
}

#[derive(Error, Debug)]
pub enum FSWriteError {
    #[error("failed to seek to write offset")]
    SeekError(#[source] JsError),
    #[error("failed to write to block device")]
    WriteError(#[source] JsError),
}

#[async_trait]
impl xdvdfs::blockdev::BlockDeviceWrite for FSWriteWrapper {
    type WriteError = FSWriteError;

    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), Self::WriteError> {
        UnsafeJSFuture::from(self.stream.seek(offset as f64))
            .await
            .map_err(JsError::from)
            .map_err(FSWriteError::SeekError)?;

        let buffer_len = buffer.len() as u64;
        let buffer = js_sys::Uint8Array::from(buffer);
        UnsafeJSFuture::from(self.stream.write_u8_array(buffer))
            .await
            .map_err(JsError::from)
            .map_err(FSWriteError::WriteError)?;

        self.len = core::cmp::max(self.len, offset + buffer_len);

        Ok(())
    }

    async fn len(&mut self) -> Result<u64, Self::WriteError> {
        Ok(self.len)
    }
}

#[derive(Error, Debug)]
pub enum FSReadError {
    #[error("failed to convert into file handle")]
    FileHandleError(#[source] JsError),
    #[error("failed to seek to read offset")]
    SeekError(#[source] JsError),
    #[error("failed to convert to array buffer")]
    ArrayBufferError(#[source] JsError),
    #[error("source buffer length length {0} does not match destination length {1}")]
    BufferSizeMismatch(u32, usize),
}

#[async_trait]
impl xdvdfs::blockdev::BlockDeviceRead for FileSystemFileHandle {
    type ReadError = FSReadError;

    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), FSReadError> {
        let offset: f64 = offset as f64;
        let size: f64 = buffer.len() as u64 as f64;

        let slice = self
            .to_file()
            .await
            .map_err(FSReadError::FileHandleError)?
            .slice_with_f64_and_f64_and_content_type(
                offset,
                offset + size,
                "application/octet-stream",
            )
            .map_err(JsError::from)
            .map_err(FSReadError::SeekError)?
            .array_buffer();
        let slice_buf = UnsafeJSFuture::from(slice)
            .await
            .map_err(JsError::from)
            .map_err(FSReadError::ArrayBufferError)?;
        let slice_buf = js_sys::Uint8Array::new(&slice_buf);

        if slice_buf.byte_length() as usize != buffer.len() {
            return Err(FSReadError::BufferSizeMismatch(
                slice_buf.byte_length(),
                buffer.len(),
            ));
        }

        slice_buf.copy_to(buffer);
        Ok(())
    }

    async fn image_size(&mut self) -> Result<u64, FSReadError> {
        let file_size = self
            .to_file()
            .await
            .map_err(FSReadError::FileHandleError)?
            .size();
        Ok(file_size as u64)
    }
}

struct FSTree {
    subtree: BTreeMap<String, FSTree>,
    handle: util::HandleType,
}

impl FSTree {
    #[async_recursion(?Send)]
    async fn populate(&mut self) -> anyhow::Result<()> {
        assert_eq!(0, self.subtree.len());

        if let util::HandleType::Directory(ref dir) = self.handle {
            let entries = dir
                .entry_list()
                .await
                .map_err(JsError::from)
                .context("Failed to fetch directory entry list")?;
            for (path, handle) in entries {
                let mut node = FSTree {
                    subtree: BTreeMap::new(),
                    handle,
                };

                node.populate().await?;
                self.subtree.insert(path, node);
            }
        }

        Ok(())
    }
}

pub struct WebFileSystem(FSTree);

unsafe impl Send for WebFileSystem {}
unsafe impl Sync for WebFileSystem {}

#[derive(Error, Debug)]
pub enum WebFileSystemHierarchyError {
    #[error("path \"{0}\" does not exist")]
    PathDoesNotExist(PathVec),
    #[error("path \"{0}\" does not point to a directory")]
    NotDirectory(PathVec),
    #[error("failed to convert into file handle")]
    FileHandleError(#[source] JsError),
}

#[async_trait]
impl xdvdfs::write::fs::FilesystemHierarchy for WebFileSystem {
    type Error = WebFileSystemHierarchyError;

    async fn read_dir(
        &mut self,
        dir: PathRef<'_>,
    ) -> Result<Vec<xdvdfs::write::fs::FileEntry>, Self::Error> {
        let entries = self.entries(dir).await?;
        let mut file_entries = Vec::new();

        for (name, handle) in entries {
            let entry = match handle {
                util::HandleType::File(fh) => {
                    let file = fh
                        .to_file()
                        .await
                        .map_err(WebFileSystemHierarchyError::FileHandleError)?;
                    xdvdfs::write::fs::FileEntry {
                        name,
                        file_type: xdvdfs::write::fs::FileType::File,
                        len: file.size() as u64,
                    }
                }
                util::HandleType::Directory(_) => xdvdfs::write::fs::FileEntry {
                    name,
                    file_type: xdvdfs::write::fs::FileType::Directory,
                    len: 0,
                },
            };

            file_entries.push(entry);
        }

        Ok(file_entries)
    }
}

#[derive(Error, Debug)]
pub enum WebFileSystemCopierError {
    #[error("path \"{0}\" does not exist")]
    PathDoesNotExist(PathVec),
    #[error("path \"{0}\" does not point to a file")]
    NotFile(PathVec),
    #[error("failed to seek to read offset")]
    SeekError(#[source] JsError),
    #[error("failed to convert source file into file handle")]
    FileHandleError(#[source] JsError),
    #[error("failed to seek to source read position {0} (size {1})")]
    SourceSliceError(u64, u64, #[source] JsError),
    #[error("failed to copy data into output")]
    CopyDataError(#[source] JsError),
}

#[async_trait]
impl xdvdfs::write::fs::FilesystemCopier<FSWriteWrapper> for WebFileSystem {
    type Error = WebFileSystemCopierError;

    async fn copy_file_in(
        &mut self,
        src: PathRef<'_>,
        dest: &mut FSWriteWrapper,
        input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, Self::Error> {
        let src_node = self
            .walk(src)
            .ok_or_else(|| WebFileSystemCopierError::PathDoesNotExist(src.into()))?;
        let util::HandleType::File(ref src_fh) = src_node.handle else {
            return Err(WebFileSystemCopierError::NotFile(src.into()));
        };

        UnsafeJSFuture::from(dest.stream.seek(output_offset as f64))
            .await
            .map_err(JsError::from)
            .map_err(WebFileSystemCopierError::SeekError)?;

        let (file_size, write_promise) = src_fh
            .to_file()
            .await
            .map_err(WebFileSystemCopierError::FileHandleError)
            .and_then(|file| {
                file.slice_with_f64_and_f64_and_content_type(
                    input_offset as f64,
                    (input_offset + size) as f64,
                    "application/octet-stream",
                )
                .map_err(JsError::from)
                .map_err(|e| WebFileSystemCopierError::SourceSliceError(input_offset, size, e))
            })
            .map(|blob| (blob.size() as u64, dest.stream.write_blob(blob)))?;
        assert_eq!(file_size, size);

        UnsafeJSFuture::from(write_promise)
            .await
            .map_err(JsError::from)
            .map_err(WebFileSystemCopierError::CopyDataError)?;
        dest.len = core::cmp::max(dest.len, output_offset + file_size);

        Ok(file_size)
    }
}

#[async_trait]
impl xdvdfs::write::fs::FilesystemCopier<[u8]> for WebFileSystem {
    type Error = WebFileSystemCopierError;

    async fn copy_file_in(
        &mut self,
        src: PathRef<'_>,
        dest: &mut [u8],
        input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, Self::Error> {
        let src_node = self
            .walk(src)
            .ok_or_else(|| WebFileSystemCopierError::PathDoesNotExist(src.into()))?;
        let size = core::cmp::min(size, dest.len() as u64);
        let util::HandleType::File(ref src_fh) = src_node.handle else {
            return Err(WebFileSystemCopierError::NotFile(src.into()));
        };

        let slice = src_fh
            .to_file()
            .await
            .map_err(WebFileSystemCopierError::FileHandleError)
            .and_then(|file| {
                let file_size = file.size() as u64;
                let size = core::cmp::min(file_size, size);
                file.slice_with_f64_and_f64_and_content_type(
                    input_offset as f64,
                    input_offset as f64 + size as f64,
                    "application/octet-stream",
                )
                .map_err(JsError::from)
                .map_err(|e| WebFileSystemCopierError::SourceSliceError(input_offset, size, e))
            })?
            .array_buffer();

        let slice_buf = UnsafeJSFuture::from(slice)
            .await
            .map_err(JsError::from)
            .map_err(WebFileSystemCopierError::CopyDataError)?;
        let slice_buf = js_sys::Uint8Array::new(&slice_buf);

        // Now that we have the slice, readjust expected copy size
        let output_offset = output_offset as usize;
        let size = core::cmp::min(dest.len() - output_offset, slice_buf.byte_length() as usize);
        slice_buf.copy_to(&mut dest[output_offset..(output_offset + size)]);

        if size != dest.len() - output_offset {
            dest[(output_offset + size)..].fill(0);
        }

        Ok(dest.len() as u64)
    }
}

impl WebFileSystem {
    pub async fn new(root_handle: FileSystemDirectoryHandle) -> anyhow::Result<Self> {
        let mut root = FSTree {
            subtree: BTreeMap::new(),
            handle: util::HandleType::Directory(root_handle),
        };

        root.populate().await?;
        Ok(Self(root))
    }

    fn walk(&self, path: PathRef<'_>) -> Option<&FSTree> {
        let mut node = &self.0;

        for component in &path {
            node = node.subtree.get(component)?;
        }

        Some(node)
    }

    async fn entries(
        &self,
        path: PathRef<'_>,
    ) -> Result<Vec<(String, util::HandleType)>, WebFileSystemHierarchyError> {
        let node = &self
            .walk(path)
            .ok_or_else(|| WebFileSystemHierarchyError::PathDoesNotExist(path.into()))?;
        if let util::HandleType::Directory(_) = node.handle {
            Ok(node
                .subtree
                .iter()
                .map(|(path, node)| {
                    let path = path.to_string();
                    (path, node.handle.clone())
                })
                .collect())
        } else {
            Err(WebFileSystemHierarchyError::NotDirectory(path.into()))
        }
    }
}
