use async_recursion::async_recursion;
use async_trait::async_trait;
use js_sys::JsString;
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;
use xdvdfs::write::fs::PathVec;

pub mod bindings;
pub use bindings::*;
pub mod ciso;

mod util;
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

#[async_trait]
impl xdvdfs::blockdev::BlockDeviceWrite<String> for FSWriteWrapper {
    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), String> {
        UnsafeJSFuture::from(self.stream.seek(offset as f64))
            .await
            .map_err(|_| "Failed to seek")?;

        let buffer_len = buffer.len() as u64;
        let buffer = js_sys::Uint8Array::from(buffer);
        UnsafeJSFuture::from(self.stream.write_u8_array(buffer))
            .await
            .map_err(|_| "Failed to write")?;

        self.len = core::cmp::max(self.len, offset + buffer_len);

        Ok(())
    }

    async fn len(&mut self) -> Result<u64, String> {
        Ok(self.len)
    }
}

#[async_trait]
impl xdvdfs::blockdev::BlockDeviceRead<String> for FileSystemFileHandle {
    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), String> {
        let offset: f64 = offset as f64;
        let size: f64 = buffer.len() as u64 as f64;

        let slice = self
            .to_file()
            .await?
            .slice_with_f64_and_f64_and_content_type(
                offset,
                offset + size,
                "application/octet-stream",
            )
            .map_err(|_| "failed to slice")?
            .array_buffer();
        let slice_buf = UnsafeJSFuture::from(slice)
            .await
            .map_err(|_| "failed to obtain array buffer")?;
        let slice_buf = js_sys::Uint8Array::new(&slice_buf);

        if slice_buf.byte_length() as usize != buffer.len() {
            return Err(String::from("Not the right length"));
        }

        slice_buf.copy_to(buffer);
        Ok(())
    }
}

struct FSTree {
    subtree: BTreeMap<String, FSTree>,
    handle: util::HandleType,
}

impl FSTree {
    #[async_recursion(?Send)]
    async fn populate(&mut self) -> Result<(), String> {
        assert_eq!(0, self.subtree.len());

        if let util::HandleType::Directory(ref dir) = self.handle {
            let entries = dir
                .entry_list()
                .await
                .map_err(|_| "Failed to fetch entry list")?;
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

#[async_trait]
impl xdvdfs::write::fs::Filesystem<FSWriteWrapper, String> for WebFileSystem {
    async fn read_dir(
        &mut self,
        dir: &PathVec,
    ) -> Result<Vec<xdvdfs::write::fs::FileEntry>, String> {
        let entries = self
            .entries(dir)
            .await
            .map_err(|_| "Couldn't get the entries")?;
        let mut file_entries = Vec::new();

        for (name, handle) in entries {
            let entry = match handle {
                util::HandleType::File(fh) => {
                    let file = fh.to_file().await.map_err(|_| "Couldn't get the file")?;
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

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut FSWriteWrapper,
        offset: u64,
        _size: u64,
    ) -> Result<u64, String> {
        let src_node = self.walk(src).ok_or("Failed to find src")?;
        if let util::HandleType::File(ref src_fh) = src_node.handle {
            UnsafeJSFuture::from(dest.stream.seek(offset as f64))
                .await
                .map_err(|_| "Failed to seek")?;

            let (file_size, write_promimse) = src_fh
                .to_file()
                .await
                .map_err(|_| "Failed to get file from handle")
                .map(|file| (file.size() as u64, dest.stream.write_file(file)))?;

            UnsafeJSFuture::from(write_promimse)
                .await
                .map_err(|_| "Failed to write file")?;
            dest.len = core::cmp::max(dest.len, offset + file_size);

            Ok(file_size)
        } else {
            Err(String::from("Not a file"))
        }
    }

    async fn copy_file_buf(
        &mut self,
        src: &PathVec,
        buf: &mut [u8],
        offset: u64,
    ) -> Result<u64, String> {
        let src_node = self.walk(src).ok_or("Failed to find src")?;
        if let util::HandleType::File(ref src_fh) = src_node.handle {
            let offset = offset as f64;
            let slice = src_fh
                .to_file()
                .await
                .map_err(|_| "Failed to get file from handle")
                .and_then(|file| {
                    let file_size = file.size() as u64;
                    let size = core::cmp::min(file_size, buf.len() as u64);
                    file.slice_with_f64_and_f64_and_content_type(
                        offset,
                        offset + size as f64,
                        "application/octet-stream",
                    )
                    .map_err(|_| "failed to slice")
                })?
                .array_buffer();

            let slice_buf = UnsafeJSFuture::from(slice)
                .await
                .map_err(|_| "failed to obtain array buffer")?;
            let slice_buf = js_sys::Uint8Array::new(&slice_buf);

            // Now that we have the slice, readjust expected copy size
            let size = core::cmp::min(buf.len(), slice_buf.byte_length() as usize);
            slice_buf.copy_to(&mut buf[0..size]);

            if size != buf.len() {
                buf[size..].fill(0);
            }

            Ok(buf.len() as u64)
        } else {
            Err(String::from("Not a file"))
        }
    }
}

impl WebFileSystem {
    pub async fn new(root_handle: FileSystemDirectoryHandle) -> Self {
        let mut root = FSTree {
            subtree: BTreeMap::new(),
            handle: util::HandleType::Directory(root_handle),
        };

        root.populate().await.unwrap();
        Self(root)
    }

    fn walk(&self, path: &PathVec) -> Option<&FSTree> {
        let mut node = &self.0;

        for component in path.iter() {
            node = node.subtree.get(component)?;
        }

        Some(node)
    }

    async fn entries(&self, path: &PathVec) -> Result<Vec<(String, util::HandleType)>, JsValue> {
        let node = &self.walk(path).unwrap();
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
            Err(JsString::from("Not a directory").into())
        }
    }
}
