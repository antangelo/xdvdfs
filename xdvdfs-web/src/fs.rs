use async_recursion::async_recursion;
use async_trait::async_trait;
use js_sys::{Array, JsString, Object};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Component, Path},
};
use wasm_bindgen::prelude::*;

use web_sys::WritableStream;

#[wasm_bindgen]
#[derive(Clone)]
pub struct FileOptions {
    create: bool,
}

#[wasm_bindgen]
impl FileOptions {
    #[wasm_bindgen(constructor)]
    pub fn new(create: bool) -> Self {
        Self { create }
    }

    #[wasm_bindgen(getter)]
    pub fn create(&self) -> bool {
        self.create
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = Object, js_name = FileSystemHandle, typescript_type = "FileSystemHandle")]
    #[derive(Clone, PartialEq, Eq)]
    pub type FileSystemHandle;

    #[wasm_bindgen(structural, method, getter, js_class = "FileSystemHandle", js_name = name)]
    pub fn name(this: &FileSystemHandle) -> String;

    #[wasm_bindgen(extends = FileSystemHandle, extends = Object, js_name = FileSystemDirectoryHandle, typescript_type = "FileSystemDirectoryHandle")]
    #[derive(Clone, PartialEq, Eq)]
    pub type FileSystemDirectoryHandle;

    #[wasm_bindgen(method, structural, js_class = "FileSystemDirectoryHandle", js_name = entries)]
    pub fn entries(this: &FileSystemDirectoryHandle) -> js_sys::AsyncIterator;

    #[wasm_bindgen(method, structural, js_class = "FileSystemDirectoryHandle", js_name = getDirectoryHandle)]
    pub fn get_directory_handle(this: &FileSystemDirectoryHandle, name: String) -> js_sys::Promise;

    #[wasm_bindgen(method, structural, js_class = "FileSystemDirectoryHandle", js_name = getDirectoryHandle)]
    pub fn get_directory_handle_with_opts(
        this: &FileSystemDirectoryHandle,
        name: String,
        opts: FileOptions,
    ) -> js_sys::Promise;

    #[wasm_bindgen(method, structural, js_class = "FileSystemDirectoryHandle", js_name = getFileHandle)]
    pub fn get_file_handle(this: &FileSystemDirectoryHandle, name: String) -> js_sys::Promise;

    #[wasm_bindgen(method, structural, js_class = "FileSystemDirectoryHandle", js_name = getFileHandle)]
    pub fn get_file_handle_with_opts(
        this: &FileSystemDirectoryHandle,
        name: String,
        opts: FileOptions,
    ) -> js_sys::Promise;

    #[wasm_bindgen(extends = FileSystemHandle, extends = Object, js_name = FileSystemFileHandle, typescript_type = "FileSystemFileHandle")]
    #[derive(Clone, PartialEq, Eq)]
    pub type FileSystemFileHandle;

    #[wasm_bindgen(method, structural, js_class = "FileSystemFileHandle", js_name = getFile)]
    pub fn get_file(this: &FileSystemFileHandle) -> js_sys::Promise;

    #[wasm_bindgen(method, structural, js_class = "FileSystemFileHandle", js_name = createWritable)]
    pub fn create_writable(this: &FileSystemFileHandle) -> js_sys::Promise;

    #[wasm_bindgen(extends = WritableStream, extends = Object, js_name = FileSystemWritableFileStream, typescript_type = "FileSystemWritableFileStream")]
    #[derive(Clone)]
    pub type FileSystemWritableFileStream;

    #[wasm_bindgen(method, structural, js_class = "FileSystemWritableFileStream", js_name = write)]
    pub fn write_u8_array(
        this: &FileSystemWritableFileStream,
        data: js_sys::Uint8Array,
    ) -> js_sys::Promise;

    #[wasm_bindgen(method, structural, js_class = "FileSystemWritableFileStream", js_name = write)]
    pub fn write_file(this: &FileSystemWritableFileStream, data: web_sys::File) -> js_sys::Promise;

    #[wasm_bindgen(method, structural, js_class = "FileSystemWritableFileStream", js_name = seek)]
    pub fn seek(this: &FileSystemWritableFileStream, position: f64) -> js_sys::Promise;
}

impl FileSystemFileHandle {
    pub async fn writable_stream(&self) -> Result<FileSystemWritableFileStream, String> {
        let stream = wasm_bindgen_futures::JsFuture::from(self.create_writable())
            .await
            .map_err(|_| "Failed to get writable stream")?;
        let stream = FileSystemWritableFileStream::from(stream);
        Ok(stream)
    }

    pub async fn to_file(&self) -> Result<web_sys::File, String> {
        let file = wasm_bindgen_futures::JsFuture::from(self.get_file())
            .await
            .map_err(|_| "Failed to get file")?;
        let file = web_sys::File::from(file);

        Ok(file)
    }
}

impl FileSystemDirectoryHandle {
    async fn entry_list(&self) -> Result<Vec<(String, HandleType)>, JsValue> {
        let entries = self.entries();
        let mut vec = Vec::new();

        loop {
            match entries.next() {
                Ok(val) => {
                    let val = wasm_bindgen_futures::JsFuture::from(val).await;
                    match val {
                        Ok(val) => {
                            let done = js_sys::Reflect::get(&val, &js_sys::JsString::from("done"))
                                .unwrap()
                                .as_bool()
                                .unwrap();
                            if done {
                                break Ok(vec);
                            }

                            let val = js_sys::Reflect::get(&val, &js_sys::JsString::from("value"))
                                .unwrap();
                            let val: Array = val.into();

                            let path = val.get(0).as_string().unwrap();
                            let handle = val.get(1);

                            let kind = js_sys::Reflect::get(&handle, &JsValue::from("kind"))
                                .unwrap()
                                .as_string()
                                .unwrap();
                            let handle = match kind.as_str() {
                                "file" => HandleType::File(FileSystemFileHandle::from(handle)),
                                "directory" => {
                                    HandleType::Directory(FileSystemDirectoryHandle::from(handle))
                                }
                                _ => break Err(JsValue::from("Invalid kind")),
                            };

                            vec.push((path, handle));
                        }
                        Err(e) => {
                            break Err(e);
                        }
                    }
                }
                Err(e) => {
                    break Err(e);
                }
            }
        }
    }

    pub async fn create_file(&self, name: String) -> Result<FileSystemFileHandle, JsValue> {
        let opts = FileOptions::new(true);
        let handle = self.get_file_handle_with_opts(name, opts);
        let handle = wasm_bindgen_futures::JsFuture::from(handle).await?;
        Ok(FileSystemFileHandle::from(handle))
    }

    pub async fn create_directory(
        &self,
        name: String,
    ) -> Result<FileSystemDirectoryHandle, JsValue> {
        let opts = FileOptions::new(true);
        let handle = self.get_directory_handle_with_opts(name, opts);
        let handle = wasm_bindgen_futures::JsFuture::from(handle).await?;
        Ok(FileSystemDirectoryHandle::from(handle))
    }
}

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

#[derive(Clone)]
enum HandleType {
    File(FileSystemFileHandle),
    Directory(FileSystemDirectoryHandle),
}

struct TrieNode {
    subtree: BTreeMap<OsString, TrieNode>,
    handle: HandleType,
}

impl TrieNode {
    #[async_recursion(?Send)]
    async fn populate(&mut self) -> Result<(), String> {
        assert_eq!(0, self.subtree.len());

        if let HandleType::Directory(ref dir) = self.handle {
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
                HandleType::File(fh) => {
                    let file = fh.to_file().await.map_err(|_| "Couldn't get the file")?;
                    xdvdfs::write::fs::FileEntry {
                        path: dir.join(path),
                        file_type: xdvdfs::write::fs::FileType::File,
                        len: file.size() as u64,
                    }
                }
                HandleType::Directory(_) => xdvdfs::write::fs::FileEntry {
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
        if let HandleType::File(ref src_fh) = src_node.handle {
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
            handle: HandleType::Directory(root_handle),
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

    async fn entries(&self, path: &Path) -> Result<Vec<(String, HandleType)>, JsValue> {
        let node = &self.walk(path).unwrap();
        if let HandleType::Directory(_) = node.handle {
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
