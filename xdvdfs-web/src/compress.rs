use std::path::PathBuf;

use crate::fs::{FSWriteWrapper, FileSystemDirectoryHandle};

use super::fs;
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
    Compressing = 4,
    Finished = 5,
    Error(String) = 6,
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
            self >= &Self::Compressing
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::SelectInputType => "Select input type",
            Self::SelectInput => "Select input",
            Self::SelectOutput => "Select output",
            Self::Packing(ics) => ics.as_str(),
            Self::Compressing => "Compressing image (this may take a while)",
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
    output_dir_handle: Option<FileSystemDirectoryHandle>,

    packing_file_count: u32,
    packing_file_progress: u32,
    packing_file_name: Option<String>,
}

pub enum WorkflowMessage {
    DoNothing,
    UpdateProgress(ProgressInfo),
    UpdateCompressionProgress(ciso::write::ProgressInfo),
    SetInputType(InputHandleType),
    SetInput(PickerResult),
    SetOutputFile(FileSystemDirectoryHandle),
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
            self.output_dir_handle = None;
        }

        if workflow_state <= 3 {
            self.packing_file_count = 0;
            self.packing_file_progress = 0;
            self.packing_file_name = None;
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
                            disabled={is_packing}
                            onclick={ctx.link().callback(|_| WorkflowMessage::SetInputType(InputHandleType::Directory))}
                        >{"Folder"}</Button>
                        <Button
                            icon={Icon::Document}
                            disabled={is_packing}
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
                        <H5>{"Select folder to output CISO parts"}</H5>
                        <div>
                            <FilePickerButton
                                kind={PickerKind::OpenDirectory}
                                button_text={"Select output directory"}
                                disabled={is_packing}
                                setter={ctx.link().callback(|res| {
                                    if let PickerResult::DirectoryHandle(dh) = res {
                                        WorkflowMessage::SetOutputFile(dh)
                                    } else {
                                        WorkflowMessage::DoNothing
                                    }
                                })}
                            />
                            if let Some(ref dh) = self.output_dir_handle {
                                {format!("Selected: {}", dh.name())}
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
                    <H5>{"Generating CISO Image"}</H5>
                    <ProgressBar
                        value={progress}
                        animate=true
                        stripes={self.workflow_state.is_packing_and(|ics| ics == ImageCreationState::WaitingForFlush) || self.workflow_state == WorkflowState::Compressing}
                    />
                    {self.workflow_state.as_str()}
                    <br/>
                    if let WorkflowState::Error(ref e) = self.workflow_state {
                        {e}
                    } else if let WorkflowState::Compressing = self.workflow_state {
                        {format!("Compressing sector {} of {}", self.packing_file_progress, self.packing_file_count)}
                    } else {
                        {format!("Packing {} of {} files", self.packing_file_progress, self.packing_file_count)}
                    }
                        <br />
                        if let Some(ref name) = self.packing_file_name {
                            {format!("Packing {}", name)}
                        }
                    //}
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
            WorkflowMessage::SetOutputFile(dh) => {
                self.reset_state(2);
                self.output_dir_handle = Some(dh.clone());
                self.workflow_state =
                    WorkflowState::Packing(ImageCreationState::CreatingFilesystem);

                wasm_bindgen_futures::spawn_local(create_image(
                    self.input_handle.clone().unwrap(),
                    dh,
                    ctx.link().callback(WorkflowMessage::UpdateProgress),
                    ctx.link()
                        .callback(WorkflowMessage::UpdateCompressionProgress),
                    ctx.link().callback(WorkflowMessage::ChangeState),
                ));
            }
            WorkflowMessage::UpdateProgress(pi) => match pi {
                ProgressInfo::FinishedPacking => {
                    self.workflow_state =
                        WorkflowState::Packing(ImageCreationState::WaitingForFlush);
                }
                ProgressInfo::FileCount(total) => self.packing_file_count = total as u32,
                ProgressInfo::FileAdded(path, size) => {
                    self.packing_file_name = Some(format!("{:?} ({} bytes)", path, size));
                    self.packing_file_progress += 1;
                }
                _ => {}
            },
            WorkflowMessage::UpdateCompressionProgress(pi) => match pi {
                ciso::write::ProgressInfo::SectorCount(count) => {
                    self.packing_file_count = count as u32;
                    self.packing_file_name = None;
                    self.packing_file_progress = 0;
                }
                ciso::write::ProgressInfo::SectorFinished => {
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
    dest: FileSystemDirectoryHandle,
    progress_callback: yew::Callback<ProgressInfo, ()>,
    compression_progress_callback: yew::Callback<ciso::write::ProgressInfo, ()>,
    state_change_callback: &yew::Callback<WorkflowState, ()>,
) -> Result<(), String> {
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
        compression_progress_callback.emit(pi)
    })
    .await
    .map_err(|e| format!("{:?}", e))?;

    state_change_callback.emit(WorkflowState::Packing(ImageCreationState::WaitingForFlush));

    output.close().await;

    Ok(())
}

async fn create_image(
    src: PickerResult,
    dest: FileSystemDirectoryHandle,
    progress_callback: yew::Callback<ProgressInfo, ()>,
    compression_progress_callback: yew::Callback<ciso::write::ProgressInfo, ()>,
    state_change_callback: yew::Callback<WorkflowState, ()>,
) {
    let result = create_image_result(
        src,
        dest,
        progress_callback,
        compression_progress_callback,
        &state_change_callback,
    )
    .await;
    let state = match result {
        Ok(_) => WorkflowState::Finished,
        Err(e) => WorkflowState::Error(e),
    };

    state_change_callback.emit(state);
}
