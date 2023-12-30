use std::path::PathBuf;

use async_trait::async_trait;
use xdvdfs::{layout::DirectoryEntryNode, write::img::ProgressInfo};

use crate::{
    fs::{self, FSWriteWrapper, FileSystemDirectoryHandle, FileSystemFileHandle},
    picker::{browser::WebFSBackend, PickerResult},
};

use super::XDVDFSOperations;

#[derive(Eq, PartialEq, Default, Copy, Clone)]
pub struct WebXDVDFSOps;

#[async_trait(?Send)]
impl XDVDFSOperations<WebFSBackend> for WebXDVDFSOps {
    async fn pack_image(
        src: PickerResult<WebFSBackend>,
        dest: FileSystemFileHandle,
        progress_callback: yew::Callback<ProgressInfo>,
        state_change_callback: &yew::Callback<crate::packing::WorkflowState>,
    ) -> Result<(), String> {
        use crate::packing::{ImageCreationState, WorkflowState};
        let mut fs: Box<dyn xdvdfs::write::fs::Filesystem<FSWriteWrapper, String>> = match src {
            PickerResult::DirectoryHandle(dh) => Box::new(fs::WebFileSystem::new(dh).await),
            PickerResult::FileHandle(fh) => {
                let img = xdvdfs::blockdev::OffsetWrapper::new(fh).await?;
                let fs = xdvdfs::write::fs::XDVDFSFilesystem::new(img)
                    .await
                    .ok_or(String::from("Failed to create fs"))?;
                Box::new(fs)
            }
        };

        state_change_callback.emit(WorkflowState::Packing(ImageCreationState::PackingImage));
        let mut dest = fs::FSWriteWrapper::new(&dest).await;

        xdvdfs::write::img::create_xdvdfs_image(
            &std::path::PathBuf::from("/"),
            fs.as_mut(),
            &mut dest,
            |pi| progress_callback.emit(pi),
        )
        .await?;

        state_change_callback.emit(WorkflowState::Packing(ImageCreationState::WaitingForFlush));
        dest.close().await;

        Ok(())
    }

    async fn unpack_image(
        src: FileSystemFileHandle,
        dest: FileSystemDirectoryHandle,
        progress_callback: yew::Callback<ProgressInfo>,
        _state_change_callback: &yew::Callback<crate::unpacking::WorkflowState>,
    ) -> Result<(), String> {
        let src_file = src.to_file().await?;
        let mut img = xdvdfs::blockdev::OffsetWrapper::new(src).await?;
        let volume = xdvdfs::read::read_volume(&mut img).await?;

        let mut stack: Vec<(FileSystemDirectoryHandle, DirectoryEntryNode)> = Vec::new();
        for entry in volume.root_table.walk_dirent_tree(&mut img).await? {
            stack.push((dest.clone(), entry));
        }

        let mut file_count = stack.len();
        progress_callback.emit(ProgressInfo::FileCount(file_count));

        while let Some((parent, node)) = stack.pop() {
            let file_name = node.name_str::<String>()?.into_owned();
            if let Some(dirtab) = node.node.dirent.dirent_table() {
                let dir = parent
                    .create_directory(file_name.clone())
                    .await
                    .map_err(|_| "failed to create directory")?;
                let entries = dirtab.walk_dirent_tree(&mut img).await?;
                file_count += entries.len();
                progress_callback.emit(ProgressInfo::FileCount(file_count));

                for entry in entries {
                    stack.push((dir.clone(), entry));
                }
            } else {
                let file = parent
                    .create_file(file_name.clone())
                    .await
                    .map_err(|_| "failed to create file")?;
                if node.node.dirent.data.size == 0 {
                    continue;
                }

                let offset = node.node.dirent.data.offset::<String>(0)? as f64;
                let size = node.node.dirent.data.size as f64;
                let data = src_file
                    .slice_with_f64_and_f64_and_content_type(
                        offset,
                        offset + size,
                        "application/octet-stream",
                    )
                    .map_err(|_| "Failed to slice")?;
                let data = wasm_bindgen_futures::JsFuture::from(data.array_buffer())
                    .await
                    .map_err(|_| "Failed to obtain array buffer")?;
                let data = js_sys::Uint8Array::new(&data);
                let writeable_stream = file.writable_stream().await?;
                wasm_bindgen_futures::JsFuture::from(writeable_stream.write_u8_array(data))
                    .await
                    .map_err(|_| "Failed to write file")?;
                wasm_bindgen_futures::JsFuture::from(writeable_stream.close())
                    .await
                    .map_err(|_| "Failed to flush file")?;
            }

            // FIXME: Path
            progress_callback.emit(ProgressInfo::FileAdded(
                PathBuf::from(file_name),
                node.node.dirent.data.size as u64,
            ));
        }

        Ok(())
    }

    async fn compress_image(
        src: PickerResult<WebFSBackend>,
        dest: FileSystemDirectoryHandle,
        progress_callback: yew::Callback<ProgressInfo, ()>,
        compression_progress_callback: yew::Callback<crate::compress::CisoProgressInfo>,
        state_change_callback: &yew::Callback<crate::compress::WorkflowState>,
    ) -> Result<(), String> {
        use crate::compress::{ImageCreationState, WorkflowState};

        let (mut fs, name): (
            Box<dyn xdvdfs::write::fs::Filesystem<FSWriteWrapper, String>>,
            String,
        ) = match src {
            PickerResult::DirectoryHandle(dh) => {
                let name = dh.name();
                (Box::new(fs::WebFileSystem::new(dh).await), name)
            }
            PickerResult::FileHandle(fh) => {
                let name = fh.name();
                let img = xdvdfs::blockdev::OffsetWrapper::new(fh).await?;
                let fs = xdvdfs::write::fs::XDVDFSFilesystem::new(img)
                    .await
                    .ok_or(String::from("Failed to create fs"))?;
                (Box::new(fs), name)
            }
        };

        state_change_callback.emit(WorkflowState::Packing(ImageCreationState::PackingImage));

        let mut slbd = xdvdfs::write::fs::SectorLinearBlockDevice::default();
        let mut slbfs: xdvdfs::write::fs::SectorLinearBlockFilesystem<
            String,
            FSWriteWrapper,
            Box<dyn xdvdfs::write::fs::Filesystem<FSWriteWrapper, String>>,
        > = xdvdfs::write::fs::SectorLinearBlockFilesystem::new(&mut fs);

        xdvdfs::write::img::create_xdvdfs_image(
            &std::path::PathBuf::from("/"),
            &mut slbfs,
            &mut slbd,
            |pi| progress_callback.emit(pi),
        )
        .await?;

        state_change_callback.emit(WorkflowState::Compressing);

        let output = crate::fs::ciso::CisoOutputDirectory::new(dest);
        let mut output = ciso::split::SplitOutput::new(output, PathBuf::from(name));
        let mut input = xdvdfs::write::fs::CisoSectorInput::new(slbd, slbfs);
        ciso::write::write_ciso_image(&mut input, &mut output, |pi| {
            let pi = match pi {
                ciso::write::ProgressInfo::SectorCount(sc) => {
                    crate::compress::CisoProgressInfo::SectorCount(sc)
                }
                ciso::write::ProgressInfo::SectorFinished => {
                    crate::compress::CisoProgressInfo::SectorsDone(1)
                }
                ciso::write::ProgressInfo::Finished => crate::compress::CisoProgressInfo::Finished,
                _ => return,
            };
            compression_progress_callback.emit(pi)
        })
        .await
        .map_err(|e| format!("{:?}", e))?;

        state_change_callback.emit(WorkflowState::Packing(ImageCreationState::WaitingForFlush));

        output.close().await;

        Ok(())
    }
}
