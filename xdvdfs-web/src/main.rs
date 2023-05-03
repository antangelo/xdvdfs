use yew::prelude::*;

mod fs;
mod img_workflow;
mod info;
mod picker;

use yewprint::{Callout, Intent};

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
                    if picker::isFilePickerAvailable() {
                        <img_workflow::ImageBuilderWorkflow />
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
                        <a href={"https://github.com/antangelo/xdvdfs"}>
                            {"View on GitHub"}
                        </a>
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
