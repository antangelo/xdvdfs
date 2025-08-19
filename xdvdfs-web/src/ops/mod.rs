use async_trait::async_trait;
use xdvdfs::write::img::OwnedProgressInfo;

use crate::picker::{FilePickerBackend, PickerResult};

pub mod browser;

#[async_trait(?Send)]
pub trait XDVDFSOperations<FPB: FilePickerBackend>: Default + Clone {
    async fn pack_image(
        src: PickerResult<FPB>,
        dest: FPB::FileHandle,
        progress_callback: yew::Callback<OwnedProgressInfo>,
        state_change_callback: &yew::Callback<crate::packing::WorkflowState>,
    ) -> anyhow::Result<()>;

    async fn unpack_image(
        src: FPB::FileHandle,
        dest: FPB::DirectoryHandle,
        progress_callback: yew::Callback<OwnedProgressInfo>,
        state_change_callback: &yew::Callback<crate::unpacking::WorkflowState>,
    ) -> anyhow::Result<()>;

    async fn compress_image(
        src: PickerResult<FPB>,
        dest: FPB::DirectoryHandle,
        progress_callback: yew::Callback<OwnedProgressInfo, ()>,
        compression_progress_callback: yew::Callback<crate::compress::CisoProgressInfo>,
        state_change_callback: &yew::Callback<crate::compress::WorkflowState, ()>,
    ) -> anyhow::Result<()>;
}
