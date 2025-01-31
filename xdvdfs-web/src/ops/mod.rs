use async_trait::async_trait;
use xdvdfs::write::img::ProgressInfo;

use crate::picker::{FilePickerBackend, PickerResult};

#[cfg(not(feature = "tauri"))]
pub mod browser;

#[cfg(feature = "tauri")]
pub mod tauri;

#[async_trait(?Send)]
pub trait XDVDFSOperations<FPB: FilePickerBackend>: Default + Clone {
    async fn pack_image(
        src: PickerResult<FPB>,
        dest: FPB::FileHandle,
        progress_callback: yew::Callback<ProgressInfo>,
        state_change_callback: &yew::Callback<crate::packing::WorkflowState>,
    ) -> Result<(), String>;

    async fn unpack_image(
        src: FPB::FileHandle,
        dest: FPB::DirectoryHandle,
        progress_callback: yew::Callback<ProgressInfo>,
        state_change_callback: &yew::Callback<crate::unpacking::WorkflowState>,
    ) -> Result<(), String>;

    async fn compress_image(
        src: PickerResult<FPB>,
        dest: FPB::DirectoryHandle,
        progress_callback: yew::Callback<ProgressInfo, ()>,
        compression_progress_callback: yew::Callback<crate::compress::CisoProgressInfo>,
        state_change_callback: &yew::Callback<crate::compress::WorkflowState, ()>,
    ) -> Result<(), String>;
}
