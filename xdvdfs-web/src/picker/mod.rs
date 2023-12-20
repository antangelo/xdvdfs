use yew::prelude::*;
use yewprint::Button;

#[cfg(not(feature = "tauri"))]
pub mod browser;

//#[cfg(feature = "tauri")]
pub mod tauri;

pub fn is_file_picker_available() -> bool {
    #[cfg(not(feature = "tauri"))]
    return browser::isFilePickerAvailable();

    #[cfg(feature = "tauri")]
    true
}

pub trait FilePickerBackend: PartialEq + Eq + Clone + Default {
    type FileHandle;
    type DirectoryHandle;

    fn open_file_picker(callback: Box<dyn Fn(Self::FileHandle)>);
    fn save_file_picker(callback: Box<dyn Fn(Self::FileHandle)>, suggested_name: Option<String>);
    fn open_directory_picker(callback: Box<dyn Fn(Self::DirectoryHandle)>);

    fn dir_name(dh: &Self::DirectoryHandle) -> String;
    fn file_name(fh: &Self::FileHandle) -> String;

    // Workaround for associated type bounds being unstable
    fn clone_dir_handle(dh: &Self::DirectoryHandle) -> Self::DirectoryHandle;
    fn clone_file_handle(fh: &Self::FileHandle) -> Self::FileHandle;
}

#[derive(PartialEq)]
pub enum PickerResult<T: FilePickerBackend> {
    FileHandle(T::FileHandle),
    DirectoryHandle(T::DirectoryHandle),
}

impl<FPB: FilePickerBackend> Clone for PickerResult<FPB> {
    fn clone(&self) -> Self {
        match self {
            Self::FileHandle(fh) => Self::FileHandle(FPB::clone_file_handle(fh)),
            Self::DirectoryHandle(dh) => Self::DirectoryHandle(FPB::clone_dir_handle(dh)),
        }
    }
}

#[derive(PartialEq)]
pub enum PickerKind {
    OpenFile,
    OpenDirectory,
    SaveFile(Option<String>),
}

#[derive(Properties, PartialEq)]
pub struct PickerProps<T: FilePickerBackend + PartialEq> {
    #[prop_or_default]
    pub setter: Callback<PickerResult<T>, ()>,

    #[prop_or_default]
    pub button_text: String,

    pub kind: PickerKind,

    #[prop_or_default]
    pub disabled: bool,
}

pub enum PickerMessage<T: FilePickerBackend> {
    ShowPickerDialog,
    PickedFile(T::FileHandle),
    PickedDirectory(T::DirectoryHandle),
}

pub struct FilePickerButton<T: FilePickerBackend> {
    backend_type: core::marker::PhantomData<T>,
}

impl<T: FilePickerBackend + PartialEq + 'static> Component for FilePickerButton<T> {
    type Message = PickerMessage<T>;
    type Properties = PickerProps<T>;

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
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

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            backend_type: core::marker::PhantomData,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            PickerMessage::ShowPickerDialog => {
                match ctx.props().kind {
                    PickerKind::OpenFile => {
                        let cb = ctx.link().callback(PickerMessage::PickedFile);
                        let cb = Box::new(move |f| cb.emit(f));
                        T::open_file_picker(cb);
                    }
                    PickerKind::OpenDirectory => {
                        let cb = ctx.link().callback(PickerMessage::PickedDirectory);
                        let cb = Box::new(move |f| cb.emit(f));
                        T::open_directory_picker(cb);
                    }
                    PickerKind::SaveFile(ref suggested_name) => {
                        let cb = ctx.link().callback(PickerMessage::PickedFile);
                        let cb = Box::new(move |f| cb.emit(f));
                        T::save_file_picker(cb, suggested_name.clone());
                    }
                }

                false
            }
            PickerMessage::PickedFile(fh) => {
                ctx.props().setter.emit(PickerResult::FileHandle(fh));
                true
            }
            PickerMessage::PickedDirectory(dh) => {
                ctx.props().setter.emit(PickerResult::DirectoryHandle(dh));
                true
            }
        }
    }

    fn changed(&mut self, _ctx: &Context<Self>, _old_props: &Self::Properties) -> bool {
        true
    }
}
