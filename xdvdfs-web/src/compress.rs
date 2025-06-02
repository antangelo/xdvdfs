use crate::ops::XDVDFSOperations;
use crate::picker::FilePickerBackend;

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

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy)]
pub enum CisoProgressInfo {
    SectorCount(usize),
    SectorsDone(usize),
    Finished,
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

pub struct ImageBuilderWorkflow<FPB: FilePickerBackend, XO: XDVDFSOperations<FPB>> {
    workflow_state: WorkflowState,

    input_handle_type: Option<InputHandleType>,
    input_handle: Option<PickerResult<FPB>>,
    output_dir_handle: Option<FPB::DirectoryHandle>,

    packing_file_count: u32,
    packing_file_progress: u32,
    packing_file_name: Option<String>,

    xdvdfs_ops_type: core::marker::PhantomData<XO>,
}

impl<FPB, XO> Default for ImageBuilderWorkflow<FPB, XO>
where
    FPB: FilePickerBackend,
    XO: XDVDFSOperations<FPB>,
{
    fn default() -> Self {
        Self {
            workflow_state: WorkflowState::default(),

            input_handle_type: None,
            input_handle: None,
            output_dir_handle: None,

            packing_file_count: 0,
            packing_file_progress: 0,
            packing_file_name: None,

            xdvdfs_ops_type: core::marker::PhantomData,
        }
    }
}

pub enum WorkflowMessage<FPB: FilePickerBackend> {
    DoNothing,
    UpdateProgress(ProgressInfo),
    UpdateCompressionProgress(CisoProgressInfo),
    SetInputType(InputHandleType),
    SetInput(PickerResult<FPB>),
    SetOutputFile(FPB::DirectoryHandle),
    ChangeState(WorkflowState),
}

impl<FPB, XO> ImageBuilderWorkflow<FPB, XO>
where
    FPB: FilePickerBackend,
    XO: XDVDFSOperations<FPB>,
{
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
            PickerResult::DirectoryHandle(dh) => FPB::dir_name(dh),
            PickerResult::FileHandle(fh) => FPB::file_name(fh),
        })
    }
}

impl<FPB, XO> Component for ImageBuilderWorkflow<FPB, XO>
where
    FPB: FilePickerBackend + 'static,
    XO: XDVDFSOperations<FPB> + 'static,
{
    type Message = WorkflowMessage<FPB>;
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
                            <FilePickerButton<FPB>
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
                            <FilePickerButton<FPB>
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
                                {format!("Selected: {}", FPB::dir_name(dh))}
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
                self.output_dir_handle = Some(FPB::clone_dir_handle(&dh));
                self.workflow_state =
                    WorkflowState::Packing(ImageCreationState::CreatingFilesystem);

                wasm_bindgen_futures::spawn_local(create_image::<FPB, XO>(
                    self.input_handle.clone().unwrap(),
                    dh,
                    ctx.link().callback(WorkflowMessage::UpdateProgress),
                    ctx.link()
                        .callback(WorkflowMessage::UpdateCompressionProgress),
                    ctx.link().callback(WorkflowMessage::ChangeState),
                ));
            }
            WorkflowMessage::UpdateProgress(pi) => match pi {
                ProgressInfo::DiscoveredDirectory(entry_count) => {
                    self.packing_file_count += entry_count as u32;
                }
                ProgressInfo::FinishedPacking => {
                    self.workflow_state =
                        WorkflowState::Packing(ImageCreationState::WaitingForFlush);
                }
                ProgressInfo::FileCount(total) => self.packing_file_count = total as u32,
                ProgressInfo::FileAdded(path, size) => {
                    self.packing_file_name = Some(format!("{path:?} ({size} bytes)"));
                    self.packing_file_progress += 1;
                }
                _ => {}
            },
            WorkflowMessage::UpdateCompressionProgress(pi) => match pi {
                CisoProgressInfo::SectorCount(count) => {
                    self.packing_file_count = count as u32;
                    self.packing_file_name = None;
                    self.packing_file_progress = 0;
                }
                CisoProgressInfo::SectorsDone(done) => {
                    self.packing_file_progress += done as u32;
                }
                CisoProgressInfo::Finished => {
                    self.packing_file_progress = self.packing_file_count;
                }
            },
            WorkflowMessage::ChangeState(wfs) => self.workflow_state = wfs,
        }

        true
    }

    fn changed(&mut self, _ctx: &Context<Self>, _old_props: &Self::Properties) -> bool {
        true
    }
}

async fn create_image<FPB: FilePickerBackend, XO: XDVDFSOperations<FPB>>(
    src: PickerResult<FPB>,
    dest: FPB::DirectoryHandle,
    progress_callback: yew::Callback<ProgressInfo, ()>,
    compression_progress_callback: yew::Callback<CisoProgressInfo>,
    state_change_callback: yew::Callback<WorkflowState, ()>,
) {
    let result = XO::compress_image(
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
