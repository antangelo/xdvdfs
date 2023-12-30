use super::bindings::*;
use js_sys::{Array, Promise};
use std::future::Future;
use wasm_bindgen::JsValue;

#[derive(Clone)]
pub(super) enum HandleType {
    File(FileSystemFileHandle),
    Directory(FileSystemDirectoryHandle),
}

pub struct UnsafeJSFuture {
    inner: wasm_bindgen_futures::JsFuture,
}

// UNSAFE: Because the UI only runs on the main browser thread
// Send is never actually used.
unsafe impl Send for UnsafeJSFuture {}
unsafe impl Sync for UnsafeJSFuture {}

impl From<Promise> for UnsafeJSFuture {
    fn from(value: Promise) -> Self {
        let future = wasm_bindgen_futures::JsFuture::from(value);
        Self { inner: future }
    }
}

impl Future for UnsafeJSFuture {
    type Output = Result<JsValue, JsValue>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        std::pin::Pin::new(&mut self.inner).poll(cx)
    }
}

impl FileSystemFileHandle {
    pub async fn writable_stream(&self) -> Result<FileSystemWritableFileStream, String> {
        let stream = UnsafeJSFuture::from(self.create_writable())
            .await
            .map_err(|_| "Failed to get writable stream")?;
        let stream = FileSystemWritableFileStream::from(stream);
        Ok(stream)
    }

    pub async fn to_file(&self) -> Result<web_sys::File, String> {
        let file = UnsafeJSFuture::from(self.get_file())
            .await
            .map_err(|_| "Failed to get file")?;
        let file = web_sys::File::from(file);

        Ok(file)
    }
}

impl FileSystemDirectoryHandle {
    pub(super) async fn entry_list(&self) -> Result<Vec<(String, HandleType)>, JsValue> {
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
        let handle = UnsafeJSFuture::from(handle).await?;
        Ok(FileSystemFileHandle::from(handle))
    }

    pub async fn create_directory(
        &self,
        name: String,
    ) -> Result<FileSystemDirectoryHandle, JsValue> {
        let opts = FileOptions::new(true);
        let handle = self.get_directory_handle_with_opts(name, opts);
        let handle = UnsafeJSFuture::from(handle).await?;
        Ok(FileSystemDirectoryHandle::from(handle))
    }

    pub async fn remove_entry(&self, name: String) -> Result<(), JsValue> {
        let promise = self.remove_entry_promise(name);
        UnsafeJSFuture::from(promise).await?;
        Ok(())
    }
}
