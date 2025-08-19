use implicit_clone::{unsync::IArray, ImplicitClone};
use ops::XDVDFSOperations;
use picker::FilePickerBackend;
use yew::prelude::*;

mod compress;

mod fs;
mod info;
mod ops;
mod packing;
mod picker;
mod unpacking;

use yewprint::{Callout, Intent, Tab, Tabs};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
enum XisoTool {
    Packer,
    Unpacker,
    Compressor,
}

impl ImplicitClone for XisoTool {}

#[function_component]
fn XisoToolTab<FPB: FilePickerBackend + 'static, XO: XDVDFSOperations<FPB> + 'static>() -> Html {
    let selected_tab = use_state(|| XisoTool::Packer);

    let select_tab = {
        let selected_tab = selected_tab.clone();
        move |tab| selected_tab.set(tab)
    };

    html! {
        <Tabs<XisoTool>
            id="xisotool"
            animate=true
            selected_tab_id={*selected_tab}
            onchange={select_tab}
            tabs={[
                Tab {
                    disabled: false,
                    id: XisoTool::Packer,
                    title: html!{"Pack"},
                    panel: html!{ <packing::ImageBuilderWorkflow<FPB, XO> /> },
                    panel_class: Classes::default(),
                    title_class: Classes::default(),
                },
                Tab {
                    disabled: false,
                    id: XisoTool::Unpacker,
                    title: html!{"Unpack"},
                    panel: html!{ <unpacking::ImageUnpackingWorkflow<FPB, XO> /> },
                    panel_class: Classes::default(),
                    title_class: Classes::default(),
                },
                Tab {
                    disabled: false,
                    id: XisoTool::Compressor,
                    title: html!{"Compress"},
                    panel: html!{ <compress::ImageBuilderWorkflow<FPB, XO> /> },
                    panel_class: Classes::default(),
                    title_class: Classes::default(),
                },
            ].into_iter().collect::<IArray<_>>()}
        />
    }
}

#[function_component]
fn XisoPlatformView() -> Html {
    html! {
        <XisoToolTab<picker::browser::WebFSBackend, ops::browser::WebXDVDFSOps> />
    }
}

#[function_component]
fn GithubLink() -> Html {
    html! {
        <a href={"https://github.com/antangelo/xdvdfs"}>
            {"View on GitHub"}
        </a>
    }
}

#[function_component]
fn App() -> Html {
    let dark = use_state(|| {
        web_sys::window()
            .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
            .map(|x| x.matches())
            .unwrap_or(true)
    });

    let set_dark_mode = {
        let dark = dark.clone();
        move |_| {
            let val = *dark;
            dark.set(!val);
        }
    };

    html! {
        <div class={classes!(dark.then_some("xiso_bg_dark"))} style={"min-height: 100vh"}>
            <div class={classes!("xiso_main", dark.then_some("bp3-dark"), dark.then_some("xiso_dark"))}>
                <div style="grid-row: 1 / 2;">
                    <info::StaticSite darkmode={set_dark_mode} dark={*dark} />
                    if picker::is_file_picker_available() {
                        <XisoPlatformView />
                    } else {
                        <Callout title={"Unsupported Browser"} intent={Intent::Danger}>
                            <p>{"Your browser does not seem to support the filesystem access API."}</p>
                            <p>{"There is no need to file a bug report; when your browser begins
                                supporting the required functionality, this site will start working."}</p>
                            <p>{"Contact your browser developers to encourage them to add it."}</p>
                            <p>
                                {"In the mean time, you can use the standalone command line tool available on "}
                                <a href="https://github.com/antangelo/xdvdfs/releases/latest">{"GitHub"}</a>
                                {", or switch to a supported browser."}
                            </p>
                        </Callout>
                    }
                </div>
                <div style="margin-bottom: auto">
                    {format!("Version: {}-{}", env!("CARGO_PKG_VERSION"), env!("GIT_SHA"))}
                    <div>
                        <GithubLink />
                    </div>
                </div>
            </div>
        </div>
    }
}

fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<App>::new().render();
}
