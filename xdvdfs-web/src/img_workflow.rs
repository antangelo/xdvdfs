use super::fs::{self, FileSystemDirectoryHandle, FileSystemFileHandle};
use super::picker::{FilePickerButton, PickerKind, PickerResult};
use xdvdfs::write::img::ProgressInfo;

use yew::prelude::*;
use yewprint::{Callout, Intent, ProgressBar, H5};

#[derive(PartialEq, PartialOrd, Copy, Clone)]
pub enum ImageCreationState {
    CreatingFilesystem,
    PackingImage,
    WaitingForFlush,
}

impl ImageCreationState {
    fn as_str(&self) -> &str {
        match self {
            Self::CreatingFilesystem => "Creating filesystem",
            Self::PackingImage => "Packing image",
            Self::WaitingForFlush => "Waiting for browser to release output file",
        }
    }
}

#[derive(Default, PartialEq, PartialOrd, Clone)]
#[repr(u8)]
pub enum WorkflowState {
    #[default]
    SelectInput = 0,

    SelectOutput = 1,
    Packing(ImageCreationState) = 2,
    Finished = 3,
    Error(String) = 4,
}

impl WorkflowState {
    fn is_at_least(&self, other: Self) -> bool {
        self >= &other
    }

    fn is_packing_and(&self, cb: impl FnOnce(ImageCreationState) -> bool) -> bool {
        if let Self::Packing(ics) = self {
            cb(*ics)
        } else {
            false
        }
    }

    fn is_at_least_packing(&self) -> bool {
        if let Self::Packing(_) = self {
            true
        } else {
            self >= &Self::Finished
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::SelectInput => "Select input",
            Self::SelectOutput => "Select output",
            Self::Packing(ics) => ics.as_str(),
            Self::Finished => "Finished",
            Self::Error(_) => "Errored",
        }
    }
}

#[derive(Default)]
pub struct ImageBuilderWorkflow {
    workflow_state: WorkflowState,

    directory_handle: Option<FileSystemDirectoryHandle>,
    output_file_handle: Option<FileSystemFileHandle>,

    packing_file_count: u32,
    packing_file_progress: u32,
}

pub enum WorkflowMessage {
    DoNothing,
    UpdateProgress(ProgressInfo),
    SetInput(PickerResult),
    SetOutputFile(FileSystemFileHandle),
    ChangeState(WorkflowState),
}

impl ImageBuilderWorkflow {
    fn reset_state(&mut self, workflow_state: u8) {
        if workflow_state == 0 {
            self.directory_handle = None;
        }

        if workflow_state <= 1 {
            self.output_file_handle = None;
        }

        if workflow_state <= 2 {
            self.packing_file_count = 0;
            self.packing_file_progress = 0;
        }
    }
}

impl Component for ImageBuilderWorkflow {
    type Message = WorkflowMessage;
    type Properties = ();

    fn view(&self, ctx: &Context<Self>) -> Html {
        let is_packing = self.workflow_state.is_packing_and(|_| true);
        let progress = if self.packing_file_count != 0 {
            (100 * self.packing_file_progress) / self.packing_file_count
        } else {
            0
        };

        html! {
            <div>
                <Callout intent={if self.workflow_state == WorkflowState::SelectInput { Intent::Primary } else { Intent::Success }}>
                    <H5>{"Select an input folder containing Xbox software to pack"}</H5>
                    <div>
                        <FilePickerButton
                            kind={PickerKind::OpenDirectory}
                            button_text={"Select folder"}
                            disabled={is_packing}
                            setter={ctx.link().callback(WorkflowMessage::SetInput)}
                />
                    if let Some(ref dh) = self.directory_handle {
                        {format!("Selected: {}", dh.name())}
                    }
                </div>
                </Callout>
                if self.workflow_state.is_at_least(WorkflowState::SelectOutput) {
                    <Callout intent={if self.workflow_state == WorkflowState::SelectOutput { Intent::Primary } else { Intent::Success }}>
                        <H5>{"Save the output XISO image to a file"}</H5>
                        <div>
                        <FilePickerButton
                            kind={PickerKind::SaveFile(self.directory_handle.as_ref().map(|dh| format!("{}.iso", dh.name())))}
                            button_text={"Save image"}
                            disabled={is_packing}
                            setter={ctx.link().callback(|res| {
                                if let PickerResult::FileHandle(fh) = res {
                                    WorkflowMessage::SetOutputFile(fh)
                                } else {
                                    WorkflowMessage::DoNothing
                                }
                            })}
                    />
                        if let Some(ref fh) = self.output_file_handle {
                            {format!("Selected: {}", fh.name())}
                        }
                    </div>
                        </Callout>
                }
            if self.workflow_state.is_at_least_packing() {
                <Callout intent={match self.workflow_state {
                    WorkflowState::Finished => Intent::Success,
                    WorkflowState::Error(_) => Intent::Danger,
                    _ => Intent::Primary,
                }}>
                    <H5>{"Generating XISO Image"}</H5>
                    <ProgressBar
                        value={progress}
                        animate=true
                        stripes={self.workflow_state.is_packing_and(|ics| ics == ImageCreationState::WaitingForFlush)}
                    />
                    {self.workflow_state.as_str()}
                    <br/>
                    if let WorkflowState::Error(ref e) = self.workflow_state {
                        {e}
                    } else {
                        {format!("{} / {} files packed", self.packing_file_progress, self.packing_file_count)}
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
                if let PickerResult::DirectoryHandle(dh) = input {
                    self.reset_state(0);
                    self.directory_handle = Some(dh);
                    self.workflow_state = WorkflowState::SelectOutput;
                }
            }
            WorkflowMessage::SetOutputFile(fh) => {
                self.reset_state(1);
                self.output_file_handle = Some(fh.clone());
                self.workflow_state =
                    WorkflowState::Packing(ImageCreationState::CreatingFilesystem);
                let dh = self.directory_handle.clone().unwrap();

                wasm_bindgen_futures::spawn_local(create_image(
                    dh,
                    fh,
                    ctx.link().callback(WorkflowMessage::UpdateProgress),
                    ctx.link().callback(WorkflowMessage::ChangeState),
                ));
            }
            WorkflowMessage::UpdateProgress(pi) => match pi {
                ProgressInfo::FinishedPacking => {
                    self.workflow_state =
                        WorkflowState::Packing(ImageCreationState::WaitingForFlush);
                }
                ProgressInfo::FileCount(total) => self.packing_file_count = total as u32,
                ProgressInfo::FileAdded(_, _) => {
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

async fn create_image(
    src: FileSystemDirectoryHandle,
    dest: FileSystemFileHandle,
    progress_callback: yew::Callback<ProgressInfo, ()>,
    state_change_callback: yew::Callback<WorkflowState, ()>,
) {
    let webfs = fs::WebFileSystem::new(src).await;
    state_change_callback.emit(WorkflowState::Packing(ImageCreationState::PackingImage));
    let mut dest = fs::FSWriteWrapper::new(&dest).await;
    let result = xdvdfs::write::img::create_xdvdfs_image(
        &std::path::PathBuf::from("/"),
        &webfs,
        &mut dest,
        |pi| progress_callback.emit(pi),
    )
    .await;

    state_change_callback.emit(WorkflowState::Packing(ImageCreationState::WaitingForFlush));
    dest.close().await;

    let state = match result {
        Ok(_) => WorkflowState::Finished,
        Err(e) => WorkflowState::Error(e.to_string()),
    };

    state_change_callback.emit(state);
}
