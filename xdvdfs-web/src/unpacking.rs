use std::path::PathBuf;

use crate::fs::FileSystemDirectoryHandle;

use super::fs::FileSystemFileHandle;
use super::picker::{FilePickerButton, PickerKind, PickerResult};
use xdvdfs::layout::DirectoryEntryNode;
use xdvdfs::write::img::ProgressInfo;

use yew::prelude::*;
use yewprint::{Callout, Intent, ProgressBar, H5};

#[derive(Default, PartialEq, PartialOrd, Clone)]
#[repr(u8)]
pub enum WorkflowState {
    #[default]
    SelectInput = 0,

    SelectOutput = 1,
    Unpacking = 2,
    Finished = 3,
    Error(String) = 4,
}

impl WorkflowState {
    fn is_at_least(&self, other: Self) -> bool {
        self >= &other
    }
}

#[derive(Default)]
pub struct ImageUnpackingWorkflow {
    workflow_state: WorkflowState,

    input_handle: Option<FileSystemFileHandle>,
    output_handle: Option<FileSystemDirectoryHandle>,

    packing_file_count: u32,
    packing_file_progress: u32,
    packing_file_name: Option<String>,
}

pub enum WorkflowMessage {
    DoNothing,
    UpdateProgress(ProgressInfo),
    SetInput(PickerResult),
    SetOutputFile(FileSystemDirectoryHandle),
    ChangeState(WorkflowState),
}

impl ImageUnpackingWorkflow {
    fn reset_state(&mut self, workflow_state: u8) {
        if workflow_state == 0 {
            self.input_handle = None;
        }

        if workflow_state <= 1 {
            self.output_handle = None;
        }

        if workflow_state <= 2 {
            self.packing_file_name = None;
            self.packing_file_count = 0;
            self.packing_file_progress = 0;
        }
    }

    fn input_name(&self) -> Option<String> {
        self.input_handle.as_ref().map(|fh| fh.name())
    }
}

impl Component for ImageUnpackingWorkflow {
    type Message = WorkflowMessage;
    type Properties = ();

    fn view(&self, ctx: &Context<Self>) -> Html {
        let is_unpacking = self.workflow_state == WorkflowState::Unpacking;
        let progress = if self.packing_file_count != 0 {
            (100 * self.packing_file_progress) / self.packing_file_count
        } else {
            0
        };

        html! {
            <div>
                <Callout intent={if self.workflow_state == WorkflowState::SelectInput { Intent::Primary } else { Intent::Success }}>
                    <H5>{"Select an input ISO image containing Xbox software"}</H5>
                    <div>
                        <FilePickerButton
                            kind={PickerKind::OpenFile}
                            button_text={"Select file"}
                            disabled={is_unpacking}
                            setter={ctx.link().callback(WorkflowMessage::SetInput)}
                        />
                        if let Some(name) = self.input_name() {
                            {format!("Selected: {}", name)}
                        }
                    </div>
                </Callout>
                if self.workflow_state.is_at_least(WorkflowState::SelectOutput) {
                    <Callout intent={if self.workflow_state == WorkflowState::SelectOutput { Intent::Primary } else { Intent::Success }}>
                        <H5>{"Select output folder"}</H5>
                        <div>
                            <FilePickerButton
                                kind={PickerKind::OpenDirectory}
                                button_text={"Select output folder"}
                                disabled={is_unpacking}
                                setter={ctx.link().callback(|res| {
                                    if let PickerResult::DirectoryHandle(dh) = res {
                                        WorkflowMessage::SetOutputFile(dh)
                                    } else {
                                        WorkflowMessage::DoNothing
                                    }
                                })}
                            />
                            if let Some(ref fh) = self.output_handle {
                                {format!("Selected: {}", fh.name())}
                            }
                    </div>
                        </Callout>
                }
            if self.workflow_state.is_at_least(WorkflowState::Unpacking) {
                <Callout intent={match self.workflow_state {
                    WorkflowState::Finished => Intent::Success,
                    WorkflowState::Error(_) => Intent::Danger,
                    _ => Intent::Primary,
                }}>
                    <H5>{"Unpacking XISO Image"}</H5>
                    <ProgressBar
                        value={progress}
                        animate=true
                    />
                    if let WorkflowState::Error(ref e) = self.workflow_state {
                        {e}
                    } else {
                        {format!("Unpacked {} / {} files", self.packing_file_progress, self.packing_file_count)}
                    }
                    if self.workflow_state == WorkflowState::Unpacking {
                        <br />
                        if let Some(ref name) = self.packing_file_name {
                            {format!("Unpacking {}", name)}
                        }
                    }
                </Callout>
            }
            </div>
        }
    }

    fn create(_ctx: &Context<Self>) -> Self {
        Self::default()
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            WorkflowMessage::DoNothing => {}
            WorkflowMessage::SetInput(input) => {
                if let PickerResult::FileHandle(fh) = input {
                    self.reset_state(1);
                    self.input_handle = Some(fh);
                    self.workflow_state = WorkflowState::SelectOutput;
                }
            }
            WorkflowMessage::SetOutputFile(dh) => {
                self.reset_state(2);
                self.output_handle = Some(dh.clone());
                self.workflow_state = WorkflowState::Unpacking;

                wasm_bindgen_futures::spawn_local(unpack_image(
                    self.input_handle.take().unwrap(),
                    dh,
                    ctx.link().callback(WorkflowMessage::UpdateProgress),
                    ctx.link().callback(WorkflowMessage::ChangeState),
                ));
            }
            WorkflowMessage::UpdateProgress(pi) => match pi {
                ProgressInfo::FileCount(total) => self.packing_file_count = total as u32,
                ProgressInfo::FileAdded(path, size) => {
                    self.packing_file_name = Some(format!("{:?} ({} bytes)", path, size));
                    self.packing_file_progress += 1;
                }
                _ => {}
            },
            WorkflowMessage::ChangeState(wfs) => self.workflow_state = wfs,
        }

        true
    }

    fn changed(&mut self, _ctx: &Context<Self>, _old_props: &Self::Properties) -> bool {
        true
    }
}

async fn unpack_image_result(
    src: FileSystemFileHandle,
    dest: FileSystemDirectoryHandle,
    progress_callback: yew::Callback<ProgressInfo, ()>,
    _state_change_callback: &yew::Callback<WorkflowState, ()>,
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

async fn unpack_image(
    src: FileSystemFileHandle,
    dest: FileSystemDirectoryHandle,
    progress_callback: yew::Callback<ProgressInfo, ()>,
    state_change_callback: yew::Callback<WorkflowState, ()>,
) {
    let result = unpack_image_result(src, dest, progress_callback, &state_change_callback).await;
    let state = match result {
        Ok(_) => WorkflowState::Finished,
        Err(e) => WorkflowState::Error(e),
    };

    state_change_callback.emit(state);
}
