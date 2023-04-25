use yew::prelude::*;

use wasm_bindgen::prelude::*;

use yewprint::Button;

use crate::fs::{FileSystemDirectoryHandle, FileSystemFileHandle};

#[wasm_bindgen(module = "/src/picker.js")]
extern "C" {
    pub fn isFilePickerAvailable() -> bool;
    fn showOpenFilePicker(callback: JsValue, unused: JsValue);
    fn showDirectoryPicker(callback: JsValue, unused: JsValue);
    fn showSaveFilePicker(callback: JsValue, suggestedName: JsValue);

    fn console_log(jsv: JsValue);
}

#[derive(PartialEq)]
pub enum PickerResult {
    FileHandle(FileSystemFileHandle),
    DirectoryHandle(FileSystemDirectoryHandle),
}

#[derive(PartialEq)]
pub enum PickerKind {
    // This will be needed soon enough
    #[allow(unused)]
    OpenFile,

    OpenDirectory,
    SaveFile(Option<String>),
}

#[derive(Properties, PartialEq)]
pub struct PickerProps {
    #[prop_or_default]
    pub setter: Callback<PickerResult, ()>,

    #[prop_or_default]
    pub button_text: String,

    pub kind: PickerKind,

    #[prop_or_default]
    pub disabled: bool,
}

pub enum PickerMessage {
    ShowPickerDialog,
    Picked(JsValue),
}

pub struct FilePickerButton {
    open_picker_fn: fn(JsValue, JsValue),
    default_file_name: Option<String>,
}

impl Component for FilePickerButton {
    type Message = PickerMessage;
    type Properties = PickerProps;

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            if !isFilePickerAvailable() {
                <p>{"Your browser does not seem to support the File System Access API"}</p>
            } else {
                <div>
                    <Button
                        onclick={ctx.link().callback(|_| PickerMessage::ShowPickerDialog)}
                        disabled={ctx.props().disabled}
                    >
                        {&ctx.props().button_text}
                    </Button>
                </div>
            }
        }
    }

    fn create(ctx: &Context<Self>) -> Self {
        let open_picker_fn = match ctx.props().kind {
            PickerKind::OpenFile => showOpenFilePicker,
            PickerKind::SaveFile(_) => showSaveFilePicker,
            PickerKind::OpenDirectory => showDirectoryPicker,
        };

        let default_file_name = if let PickerKind::SaveFile(ref default_name) = ctx.props().kind {
            default_name.clone()
        } else {
            None
        };

        Self {
            open_picker_fn,
            default_file_name,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            PickerMessage::ShowPickerDialog => {
                let cb_pick = ctx.link().callback(PickerMessage::Picked);
                let closure_pick = Closure::wrap(Box::new(move |val: JsValue| {
                    cb_pick.emit(val);
                }) as Box<dyn Fn(JsValue)>);

                (self.open_picker_fn)(
                    closure_pick.into_js_value(),
                    self.default_file_name
                        .as_ref()
                        .map(|s| s.into())
                        .unwrap_or(JsValue::UNDEFINED),
                );
                false
            }
            PickerMessage::Picked(val) => {
                let handle = match ctx.props().kind {
                    PickerKind::SaveFile(_) | PickerKind::OpenFile => {
                        PickerResult::FileHandle(FileSystemFileHandle::from(val))
                    }
                    PickerKind::OpenDirectory => {
                        PickerResult::DirectoryHandle(FileSystemDirectoryHandle::from(val))
                    }
                };

                ctx.props().setter.emit(handle);
                true
            }
        }
    }

    fn changed(&mut self, _ctx: &Context<Self>, _old_props: &Self::Properties) -> bool {
        true
    }
}
