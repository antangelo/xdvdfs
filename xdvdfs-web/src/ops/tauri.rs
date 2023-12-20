use async_trait::async_trait;
use wasm_bindgen::prelude::*;
use xdvdfs::write::img::ProgressInfo;

use crate::picker::{tauri::TauriFSBackend, PickerResult};

use super::XDVDFSOperations;

#[wasm_bindgen(module = "/src/ops/tauri.js")]
extern "C" {
    async fn pack_image(
        source_path: String,
        dest_path: String,
        progress_callback: JsValue,
    ) -> JsValue;

    async fn unpack_image(
        source_path: String,
        dest_path: String,
        progress_callback: JsValue,
    ) -> JsValue;

    async fn compress_image(
        source_path: String,
        dest_path: String,
        progress_callback: JsValue,
        compression_progress_callback: JsValue,
    ) -> JsValue;

    pub fn open_url(url: String);
}

#[derive(Eq, PartialEq, Default, Copy, Clone)]
pub struct TauriXDVDFSOps;

#[async_trait(?Send)]
impl XDVDFSOperations<TauriFSBackend> for TauriXDVDFSOps {
    async fn pack_image(
        src: PickerResult<TauriFSBackend>,
        dest: String,
        progress_callback: yew::Callback<ProgressInfo>,
        state_change_callback: &yew::Callback<crate::packing::WorkflowState>,
    ) -> Result<(), String> {
        let src = match src {
            PickerResult::FileHandle(fh) => fh,
            PickerResult::DirectoryHandle(dh) => dh,
        };

        let progress_callback = Closure::wrap(Box::new(move |val: JsValue| {
            let pi = serde_wasm_bindgen::from_value(val).unwrap();
            progress_callback.emit(pi);
        }) as Box<dyn Fn(JsValue)>);

        use crate::packing::{ImageCreationState, WorkflowState};
        state_change_callback.emit(WorkflowState::Packing(ImageCreationState::PackingImage));

        let result = pack_image(src, dest, progress_callback.into_js_value()).await;

        match result.as_string() {
            Some(err) => Err(err),
            None => {
                assert!(result.is_null() || result.is_undefined());
                Ok(())
            }
        }
    }

    async fn unpack_image(
        src: String,
        dest: String,
        progress_callback: yew::Callback<ProgressInfo>,
        state_change_callback: &yew::Callback<crate::unpacking::WorkflowState>,
    ) -> Result<(), String> {
        let progress_callback = Closure::wrap(Box::new(move |val: JsValue| {
            let pi = serde_wasm_bindgen::from_value(val).unwrap();
            progress_callback.emit(pi);
        }) as Box<dyn Fn(JsValue)>);

        use crate::unpacking::WorkflowState;
        state_change_callback.emit(WorkflowState::Unpacking);

        let result = unpack_image(src, dest, progress_callback.into_js_value()).await;

        match result.as_string() {
            Some(err) => Err(err),
            None => {
                assert!(result.is_null() || result.is_undefined());
                Ok(())
            }
        }
    }

    async fn compress_image(
        src: PickerResult<TauriFSBackend>,
        dest: String,
        progress_callback: yew::Callback<ProgressInfo, ()>,
        compression_progress_callback: yew::Callback<crate::compress::CisoProgressInfo>,
        state_change_callback: &yew::Callback<crate::compress::WorkflowState>,
    ) -> Result<(), String> {
        let src = match src {
            PickerResult::FileHandle(fh) => fh,
            PickerResult::DirectoryHandle(dh) => dh,
        };

        let progress_callback = Closure::wrap(Box::new(move |val: JsValue| {
            let pi = serde_wasm_bindgen::from_value(val).unwrap();
            progress_callback.emit(pi);
        }) as Box<dyn Fn(JsValue)>);

        let compression_state_change_callback = state_change_callback.clone();
        let compression_progress_callback = Closure::wrap(Box::new(move |val: JsValue| {
            compression_state_change_callback.emit(crate::compress::WorkflowState::Compressing);
            let pi = serde_wasm_bindgen::from_value(val).unwrap();
            compression_progress_callback.emit(pi);
        }) as Box<dyn Fn(JsValue)>);

        use crate::compress::{WorkflowState, ImageCreationState};
        state_change_callback.emit(WorkflowState::Packing(ImageCreationState::PackingImage));

        let result = compress_image(
            src,
            dest,
            progress_callback.into_js_value(),
            compression_progress_callback.into_js_value(),
        ).await;

        match result.as_string() {
            Some(err) => Err(err),
            None => {
                assert!(result.is_null() || result.is_undefined());
                Ok(())
            }
        }
    }
}
