use std::path::{Path, PathBuf};

use crate::ops::XDVDFSOperations;
use crate::picker::FilePickerBackend;

use super::picker::{FilePickerButton, PickerKind, PickerResult};
use xdvdfs::write::img::ProgressInfo;

use yew::prelude::*;
use yewprint::{Button, ButtonGroup, Callout, Icon, Intent, ProgressBar, H5};

/// Similar to Path::with_extension, but will not overwrite the extension for
/// directories
// TODO: Replace with `Path::with_added_extension` after it stabilizes
pub fn with_extension(path: &Path, ext: &str, is_dir: bool) -> PathBuf {
    if !is_dir {
        return path.with_extension(ext);
    }

    let original_ext = path.extension();
    let Some(original_ext) = original_ext else {
        return path.with_extension(ext);
    };

    let mut new_ext = original_ext.to_owned();
    new_ext.push(".");
    new_ext.push(ext);
    path.with_extension(new_ext)
}

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

pub struct ImageBuilderWorkflow<
    FPB: FilePickerBackend + Default,
    XO: XDVDFSOperations<FPB> + Default,
> {
    workflow_state: WorkflowState,

    input_handle_type: Option<InputHandleType>,
    input_handle: Option<PickerResult<FPB>>,
    output_file_handle: Option<FPB::FileHandle>,

    packing_file_count: u32,
    packing_file_progress: u32,
    packing_file_name: Option<String>,

    xdvdfs_ops_ty: core::marker::PhantomData<XO>,
}

impl<FPB, XO> Default for ImageBuilderWorkflow<FPB, XO>
where
    FPB: FilePickerBackend + Default,
    XO: XDVDFSOperations<FPB> + Default,
{
    fn default() -> Self {
        Self {
            workflow_state: WorkflowState::default(),
            input_handle_type: None,
            input_handle: None,
            output_file_handle: None,

            packing_file_count: 0,
            packing_file_progress: 0,
            packing_file_name: None,

            xdvdfs_ops_ty: core::marker::PhantomData,
        }
    }
}

pub enum WorkflowMessage<FPB: FilePickerBackend> {
    DoNothing,
    UpdateProgress(ProgressInfo),
    SetInputType(InputHandleType),
    SetInput(PickerResult<FPB>),
    SetOutputFile(FPB::FileHandle),
    ChangeState(WorkflowState),
}

impl<FPB, XO> ImageBuilderWorkflow<FPB, XO>
where
    FPB: FilePickerBackend + Default,
    XO: XDVDFSOperations<FPB> + Default,
{
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
            self.packing_file_name = None;
        }
    }

    fn input_name(&self) -> Option<String> {
        self.input_handle.as_ref().map(|ih| match ih {
            PickerResult::DirectoryHandle(dh) => FPB::dir_name(dh),
            PickerResult::FileHandle(fh) => FPB::file_name(fh),
        })
    }

    fn is_input_directory(&self) -> bool {
        matches!(self.input_handle_type, Some(InputHandleType::Directory))
    }
}

impl<FPB, XO> Component for ImageBuilderWorkflow<FPB, XO>
where
    FPB: FilePickerBackend + Default + 'static,
    XO: XDVDFSOperations<FPB> + Default + 'static,
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
                        <H5>{"Save the output XISO image to a file"}</H5>
                        <div>
                            <FilePickerButton<FPB>
                                kind={PickerKind::SaveFile(
                                    self.input_name().map(|name|
                                        with_extension(
                                            Path::new(&name),
                                            "xiso.iso",
                                            self.is_input_directory(),
                                        )
                                        .file_name()
                                        .and_then(|name| name.to_str())
                                        .map(|name| name.to_owned())
                                        .expect("file name should be defined")
                                        ))}
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
                                {format!("Selected: {}", FPB::file_name(fh))}
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
                        {format!("Packing {} of {} files", self.packing_file_progress, self.packing_file_count)}
                    }
                    if self.workflow_state.is_packing_and(|_| true) {
                        <br />
                        if let Some(ref name) = self.packing_file_name {
                            {format!("Packing {}", name)}
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
                self.output_file_handle = Some(FPB::clone_file_handle(&fh));
                self.workflow_state =
                    WorkflowState::Packing(ImageCreationState::CreatingFilesystem);

                wasm_bindgen_futures::spawn_local(create_image::<FPB, XO>(
                    self.input_handle.clone().unwrap(),
                    fh,
                    ctx.link().callback(WorkflowMessage::UpdateProgress),
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
    dest: FPB::FileHandle,
    progress_callback: yew::Callback<ProgressInfo, ()>,
    state_change_callback: yew::Callback<WorkflowState, ()>,
) {
    let result = XO::pack_image(src, dest, progress_callback, &state_change_callback).await;
    let state = match result {
        Ok(_) => WorkflowState::Finished,
        Err(e) => WorkflowState::Error(e),
    };

    state_change_callback.emit(state);
}
