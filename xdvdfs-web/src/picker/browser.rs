use wasm_bindgen::prelude::*;

use crate::fs::{FileSystemDirectoryHandle, FileSystemFileHandle};

#[wasm_bindgen(module = "/src/picker/browser.js")]
extern "C" {
    pub fn isFilePickerAvailable() -> bool;
    fn showOpenFilePicker(callback: JsValue);
    fn showDirectoryPicker(callback: JsValue);
    fn showSaveFilePicker(callback: JsValue, suggestedName: JsValue);
}

#[derive(PartialEq, Eq, Clone, Default)]
pub struct WebFSBackend;

impl super::FilePickerBackend for WebFSBackend {
    type FileHandle = FileSystemFileHandle;
    type DirectoryHandle = FileSystemDirectoryHandle;

    fn open_file_picker(callback: Box<dyn Fn(Self::FileHandle)>) {
        let cb = Closure::wrap(Box::new(move |val: JsValue| {
            let fh = FileSystemFileHandle::from(val);
            callback(fh);
        }) as Box<dyn Fn(JsValue)>);
        showOpenFilePicker(cb.into_js_value());
    }

    fn save_file_picker(callback: Box<dyn Fn(Self::FileHandle)>, suggested_name: Option<String>) {
        let cb = Closure::wrap(Box::new(move |val: JsValue| {
            let fh = FileSystemFileHandle::from(val);
            callback(fh);
        }) as Box<dyn Fn(JsValue)>);
        showSaveFilePicker(cb.into_js_value(), suggested_name.into());
    }

    fn open_directory_picker(callback: Box<dyn Fn(Self::DirectoryHandle)>) {
        let cb = Closure::wrap(Box::new(move |val: JsValue| {
            let dh = FileSystemDirectoryHandle::from(val);
            callback(dh);
        }) as Box<dyn Fn(JsValue)>);
        showDirectoryPicker(cb.into_js_value());
    }

    fn dir_name(dh: &Self::DirectoryHandle) -> String {
        dh.name()
    }

    fn file_name(fh: &Self::FileHandle) -> String {
        fh.name()
    }

    fn clone_dir_handle(dh: &Self::DirectoryHandle) -> Self::DirectoryHandle {
        dh.clone()
    }

    fn clone_file_handle(fh: &Self::FileHandle) -> Self::FileHandle {
        fh.clone()
    }
}
