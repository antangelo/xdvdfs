use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/picker/tauri.js")]
extern "C" {
    fn showOpenFilePicker(callback: JsValue);
    fn showDirectoryPicker(callback: JsValue);
    fn showSaveFilePicker(callback: JsValue, suggestedName: JsValue);
}

#[derive(PartialEq, Eq, Default, Clone, Copy)]
pub struct TauriFSBackend;

impl super::FilePickerBackend for TauriFSBackend {
    type FileHandle = String;
    type DirectoryHandle = String;

    fn open_file_picker(callback: Box<dyn Fn(Self::FileHandle)>) {
        let cb = Closure::wrap(Box::new(move |val: JsValue| {
            let fh = val
                .as_string()
                .expect("tauri dialog should always return a string");
            callback(fh);
        }) as Box<dyn Fn(JsValue)>);
        showOpenFilePicker(cb.into_js_value());
    }

    fn save_file_picker(callback: Box<dyn Fn(Self::FileHandle)>, suggested_name: Option<String>) {
        let cb = Closure::wrap(Box::new(move |val: JsValue| {
            let fh = val
                .as_string()
                .expect("tauri dialog should always return a string");
            callback(fh);
        }) as Box<dyn Fn(JsValue)>);
        showSaveFilePicker(cb.into_js_value(), suggested_name.into());
    }

    fn open_directory_picker(callback: Box<dyn Fn(Self::DirectoryHandle)>) {
        let cb = Closure::wrap(Box::new(move |val: JsValue| {
            let dh = val
                .as_string()
                .expect("tauri dialog should always return a string");
            callback(dh);
        }) as Box<dyn Fn(JsValue)>);
        showDirectoryPicker(cb.into_js_value());
    }

    fn file_name(fh: &Self::FileHandle) -> String {
        fh.clone()
    }

    fn dir_name(dh: &Self::DirectoryHandle) -> String {
        dh.clone()
    }

    fn clone_file_handle(fh: &Self::FileHandle) -> Self::FileHandle {
        fh.clone()
    }

    fn clone_dir_handle(dh: &Self::DirectoryHandle) -> Self::DirectoryHandle {
        dh.clone()
    }
}
