use js_sys::Object;
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

    #[wasm_bindgen(method, structural, js_class = "FileSystemDirectoryHandle", js_name = removeEntry)]
    pub fn remove_entry_promise(this: &FileSystemDirectoryHandle, name: String) -> js_sys::Promise;

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
