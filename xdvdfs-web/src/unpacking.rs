use crate::ops::XDVDFSOperations;
use crate::picker::FilePickerBackend;

use super::picker::{FilePickerButton, PickerKind, PickerResult};
use xdvdfs::write::img::OwnedProgressInfo;

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

pub struct ImageUnpackingWorkflow<FPB: FilePickerBackend, XO: XDVDFSOperations<FPB>> {
    workflow_state: WorkflowState,

    input_handle: Option<FPB::FileHandle>,
    output_handle: Option<FPB::DirectoryHandle>,

    packing_file_count: u32,
    packing_file_progress: u32,
    packing_file_name: Option<String>,

    xdvdfs_ops_type: core::marker::PhantomData<XO>,
}

impl<FPB, XO> Default for ImageUnpackingWorkflow<FPB, XO>
where
    FPB: FilePickerBackend,
    XO: XDVDFSOperations<FPB>,
{
    fn default() -> Self {
        Self {
            workflow_state: WorkflowState::default(),

            input_handle: None,
            output_handle: None,

            packing_file_count: 0,
            packing_file_progress: 0,
            packing_file_name: None,

            xdvdfs_ops_type: core::marker::PhantomData,
        }
    }
}

pub enum WorkflowMessage<FPB: FilePickerBackend> {
    DoNothing,
    UpdateProgress(OwnedProgressInfo),
    SetInput(PickerResult<FPB>),
    SetOutputFile(FPB::DirectoryHandle),
    ChangeState(WorkflowState),
}

impl<FPB, XO> ImageUnpackingWorkflow<FPB, XO>
where
    FPB: FilePickerBackend,
    XO: XDVDFSOperations<FPB>,
{
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
        self.input_handle.as_ref().map(|fh| FPB::file_name(fh))
    }
}

impl<FPB, XO> Component for ImageUnpackingWorkflow<FPB, XO>
where
    FPB: FilePickerBackend + 'static,
    XO: XDVDFSOperations<FPB> + 'static,
{
    type Message = WorkflowMessage<FPB>;
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
                        <FilePickerButton<FPB>
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
                            <FilePickerButton<FPB>
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
                            if let Some(ref dh) = self.output_handle {
                                {format!("Selected: {}", FPB::dir_name(dh))}
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
                        <textarea
                            readonly={true}
                            value={Some(e.clone())}
                            class={classes!("xiso_err_textarea", "bp3-input")}
                        />
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
                self.output_handle = Some(FPB::clone_dir_handle(&dh));
                self.workflow_state = WorkflowState::Unpacking;

                wasm_bindgen_futures::spawn_local(unpack_image::<FPB, XO>(
                    self.input_handle.take().unwrap(),
                    dh,
                    ctx.link().callback(WorkflowMessage::UpdateProgress),
                    ctx.link().callback(WorkflowMessage::ChangeState),
                ));
            }
            WorkflowMessage::UpdateProgress(pi) => match pi {
                OwnedProgressInfo::FileCount(total) => self.packing_file_count = total as u32,
                OwnedProgressInfo::FileAdded(path, size) => {
                    self.packing_file_name = Some(format!("{path} ({size} bytes)"));
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

async fn unpack_image<FPB: FilePickerBackend, XO: XDVDFSOperations<FPB>>(
    src: FPB::FileHandle,
    dest: FPB::DirectoryHandle,
    progress_callback: yew::Callback<OwnedProgressInfo>,
    state_change_callback: yew::Callback<WorkflowState>,
) {
    let result = XO::unpack_image(src, dest, progress_callback, &state_change_callback).await;
    let state = match result {
        Ok(_) => WorkflowState::Finished,
        Err(e) => {
            let e = e.context("Failed to unpack image");
            WorkflowState::Error(format!("{e:?}"))
        }
    };

    state_change_callback.emit(state);
}
