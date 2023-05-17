use crate::fs::FSWriteWrapper;

use super::fs::{self, FileSystemFileHandle};
use super::picker::{FilePickerButton, PickerKind, PickerResult};
use xdvdfs::write::img::ProgressInfo;

use yew::prelude::*;
use yewprint::{Button, ButtonGroup, Callout, Icon, Intent, ProgressBar, H5};

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
    SelectInputType = 0,

    SelectInput = 1,

    SelectOutput = 2,
    Packing(ImageCreationState) = 3,
    Finished = 4,
    Error(String) = 5,
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
            Self::SelectInputType => "Select input type",
            Self::SelectInput => "Select input",
            Self::SelectOutput => "Select output",
            Self::Packing(ics) => ics.as_str(),
            Self::Finished => "Finished",
            Self::Error(_) => "Errored",
        }
    }
}

#[derive(Copy, Clone)]
pub enum InputHandleType {
    Directory,
    File,
}

impl InputHandleType {
    fn to_picker_kind(self) -> PickerKind {
        match self {
            Self::File => PickerKind::OpenFile,
            Self::Directory => PickerKind::OpenDirectory,
        }
    }

    fn to_str(self) -> &'static str {
        match self {
            Self::File => "ISO image",
            Self::Directory => "folder",
        }
    }
}

#[derive(Default)]
pub struct ImageBuilderWorkflow {
    workflow_state: WorkflowState,

    input_handle_type: Option<InputHandleType>,
    input_handle: Option<PickerResult>,
    output_file_handle: Option<FileSystemFileHandle>,

    packing_file_count: u32,
    packing_file_progress: u32,
}

pub enum WorkflowMessage {
    DoNothing,
    UpdateProgress(ProgressInfo),
    SetInputType(InputHandleType),
    SetInput(PickerResult),
    SetOutputFile(FileSystemFileHandle),
    ChangeState(WorkflowState),
}

impl ImageBuilderWorkflow {
    fn reset_state(&mut self, workflow_state: u8) {
        if workflow_state == 0 {
            self.input_handle_type = None;
        }

        if workflow_state <= 1 {
            self.input_handle = None;
        }

        if workflow_state <= 2 {
            self.output_file_handle = None;
        }

        if workflow_state <= 3 {
            self.packing_file_count = 0;
            self.packing_file_progress = 0;
        }
    }

    fn input_name(&self) -> Option<String> {
        self.input_handle.as_ref().map(|ih| match ih {
            PickerResult::DirectoryHandle(dh) => dh.name(),
            PickerResult::FileHandle(fh) => fh.name(),
        })
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
                <Callout intent={if self.workflow_state == WorkflowState::SelectInputType { Intent::Primary } else { Intent::Success }}>
                    <H5>{"Select the input source type"}</H5>
                    <ButtonGroup>
                        <Button
                            icon={Icon::FolderClose}
                            onclick={ctx.link().callback(|_| WorkflowMessage::SetInputType(InputHandleType::Directory))}
                        >{"Folder"}</Button>
                        <Button
                            icon={Icon::Document}
                            onclick={ctx.link().callback(|_| WorkflowMessage::SetInputType(InputHandleType::File))}
                        >{"ISO Image"}</Button>
                    </ButtonGroup>
                </Callout>
                if self.workflow_state.is_at_least(WorkflowState::SelectInput) {
                    <Callout intent={if self.workflow_state == WorkflowState::SelectInput { Intent::Primary } else { Intent::Success }}>
                        <H5>{format!("Select an input {} containing Xbox software to pack", self.input_handle_type.unwrap().to_str())}</H5>
                        <div>
                            <FilePickerButton
                                kind={self.input_handle_type.unwrap().to_picker_kind()}
                                button_text={format!("Select {}", self.input_handle_type.unwrap().to_str())}
                                disabled={is_packing}
                                setter={ctx.link().callback(WorkflowMessage::SetInput)}
                            />
                            if let Some(name) = self.input_name() {
                                {format!("Selected: {}", name)}
                            }
                        </div>
                    </Callout>
                }
                if self.workflow_state.is_at_least(WorkflowState::SelectOutput) {
                    <Callout intent={if self.workflow_state == WorkflowState::SelectOutput { Intent::Primary } else { Intent::Success }}>
                        <H5>{"Save the output XISO image to a file"}</H5>
                        <div>
                        <FilePickerButton
                            kind={PickerKind::SaveFile(self.input_name().map(|name| format!("{}.xiso", name)))}
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
            WorkflowMessage::SetInputType(it) => {
                self.reset_state(0);
                self.input_handle_type = Some(it);
                self.workflow_state = WorkflowState::SelectInput;
            }
            WorkflowMessage::SetInput(input) => {
                self.reset_state(1);
                self.input_handle = Some(input);
                self.workflow_state = WorkflowState::SelectOutput;
            }
            WorkflowMessage::SetOutputFile(fh) => {
                self.reset_state(2);
                self.output_file_handle = Some(fh.clone());
                self.workflow_state =
                    WorkflowState::Packing(ImageCreationState::CreatingFilesystem);

                wasm_bindgen_futures::spawn_local(create_image(
                    self.input_handle.take().unwrap(),
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

async fn create_image_result(
    src: PickerResult,
    dest: FileSystemFileHandle,
    progress_callback: yew::Callback<ProgressInfo, ()>,
    state_change_callback: &yew::Callback<WorkflowState, ()>,
) -> Result<(), String> {
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

async fn create_image(
    src: PickerResult,
    dest: FileSystemFileHandle,
    progress_callback: yew::Callback<ProgressInfo, ()>,
    state_change_callback: yew::Callback<WorkflowState, ()>,
) {
    let result = create_image_result(src, dest, progress_callback, &state_change_callback).await;
    let state = match result {
        Ok(_) => WorkflowState::Finished,
        Err(e) => WorkflowState::Error(e),
    };

    state_change_callback.emit(state);
}
