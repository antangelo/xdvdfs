use async_recursion::async_recursion;
use async_trait::async_trait;
use js_sys::JsString;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Component, Path},
};
use wasm_bindgen::prelude::*;

pub mod bindings;
pub use bindings::*;

mod util;

pub struct FSWriteWrapper {
    stream: FileSystemWritableFileStream,
    len: u64,
}

impl FSWriteWrapper {
    pub async fn new(fh: &FileSystemFileHandle) -> Self {
        let stream = fh.writable_stream().await.unwrap();
        Self { stream, len: 0 }
    }

    pub async fn close(self) {
        wasm_bindgen_futures::JsFuture::from(self.stream.close())
            .await
            .unwrap();
    }
}

#[async_trait(?Send)]
impl xdvdfs::blockdev::BlockDeviceWrite<String> for FSWriteWrapper {
    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), String> {
        wasm_bindgen_futures::JsFuture::from(self.stream.seek(offset as f64))
            .await
            .map_err(|_| "Failed to seek")?;

        let buffer_len = buffer.len() as u64;
        let buffer = js_sys::Uint8Array::from(buffer);
        wasm_bindgen_futures::JsFuture::from(self.stream.write_u8_array(buffer))
            .await
            .map_err(|_| "Failed to write")?;

        self.len = core::cmp::max(self.len, offset + buffer_len);

        Ok(())
    }

    async fn len(&mut self) -> Result<u64, String> {
        Ok(self.len)
    }
}

#[async_trait(?Send)]
impl xdvdfs::blockdev::BlockDeviceRead<String> for FileSystemFileHandle {
    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), String> {
        let file = self.to_file().await?;
        let offset: f64 = offset as f64;
        let size: f64 = buffer.len() as u64 as f64;

        let slice = file
            .slice_with_f64_and_f64_and_content_type(
                offset,
                offset + size,
                "application/octet-stream",
            )
            .map_err(|_| "failed to slice")?;
        let slice_buf = wasm_bindgen_futures::JsFuture::from(slice.array_buffer())
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

struct TrieNode {
    subtree: BTreeMap<OsString, TrieNode>,
    handle: util::HandleType,
}

impl TrieNode {
    #[async_recursion(?Send)]
    async fn populate(&mut self) -> Result<(), String> {
        assert_eq!(0, self.subtree.len());

        if let util::HandleType::Directory(ref dir) = self.handle {
            let entries = dir
                .entry_list()
                .await
                .map_err(|_| "Failed to fetch entry list")?;
            for (path, handle) in entries {
                let mut node = TrieNode {
                    subtree: BTreeMap::new(),
                    handle,
                };

                node.populate().await?;
                self.subtree.insert(OsString::from(path), node);
            }
        }

        Ok(())
    }
}

pub struct WebFileSystem(TrieNode);

#[async_trait(?Send)]
impl xdvdfs::write::fs::Filesystem<FSWriteWrapper, String> for WebFileSystem {
    async fn read_dir(&mut self, dir: &Path) -> Result<Vec<xdvdfs::write::fs::FileEntry>, String> {
        let entries = self
            .entries(dir)
            .await
            .map_err(|_| "Couldn't get the entries")?;
        let mut file_entries = Vec::new();

        for (path, handle) in entries {
            let entry = match handle {
                util::HandleType::File(fh) => {
                    let file = fh.to_file().await.map_err(|_| "Couldn't get the file")?;
                    xdvdfs::write::fs::FileEntry {
                        path: dir.join(path),
                        file_type: xdvdfs::write::fs::FileType::File,
                        len: file.size() as u64,
                    }
                }
                util::HandleType::Directory(_) => xdvdfs::write::fs::FileEntry {
                    path: dir.join(path),
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
        src: &Path,
        dest: &mut FSWriteWrapper,
        offset: u64,
    ) -> Result<u64, String> {
        let src_node = self.walk(src).ok_or("Failed to find src")?;
        if let util::HandleType::File(ref src_fh) = src_node.handle {
            let file = src_fh
                .to_file()
                .await
                .map_err(|_| "Failed to get file from handle")?;
            let file_size = file.size() as u64;

            wasm_bindgen_futures::JsFuture::from(dest.stream.seek(offset as f64))
                .await
                .map_err(|_| "Failed to seek")?;
            wasm_bindgen_futures::JsFuture::from(dest.stream.write_file(file))
                .await
                .map_err(|_| "Failed to write file")?;
            dest.len = core::cmp::max(dest.len, offset + file_size);

            Ok(file_size)
        } else {
            Err(String::from("Not a file"))
        }
    }
}

impl WebFileSystem {
    pub async fn new(root_handle: FileSystemDirectoryHandle) -> Self {
        let mut root = TrieNode {
            subtree: BTreeMap::new(),
            handle: util::HandleType::Directory(root_handle),
        };

        root.populate().await.unwrap();
        Self(root)
    }

    fn walk(&self, path: &Path) -> Option<&TrieNode> {
        let mut components = path.components().peekable();
        if let Some(Component::RootDir) = components.peek() {
            components.next();
        }

        let mut node = &self.0;

        for component in components {
            if let Component::Normal(component) = component {
                node = node.subtree.get(component)?;
            } else {
                return None;
            }
        }

        Some(node)
    }

    async fn entries(&self, path: &Path) -> Result<Vec<(String, util::HandleType)>, JsValue> {
        let node = &self.walk(path).unwrap();
        if let util::HandleType::Directory(_) = node.handle {
            Ok(node
                .subtree
                .iter()
                .map(|(path, node)| {
                    let path = path.to_string_lossy().to_string();
                    (path, node.handle.clone())
                })
                .collect())
        } else {
            Err(JsString::from("Not a directory").into())
        }
    }
}
