use yew::prelude::*;
use yewprint::{Callout, Intent, Switch, H1, H2};

#[derive(Properties, PartialEq)]
pub struct StaticSiteProps {
    pub dark: bool,
    pub darkmode: yew::Callback<(), ()>,
}

#[function_component]
pub fn StaticSite(props: &StaticSiteProps) -> Html {
    let cb = props.darkmode.clone();

    html! {
        <>
            <span style="display: flex; align-items: center; gap: 10px">
                <H1>{"XISO Packer"}</H1>

                <span style="margin-left: auto">
                    <Switch
                        label={html!("Toggle Dark Mode")}
                        align_right={true}
                        checked={props.dark}
                        onclick={move |_| cb.emit(())}
                    />
                </span>
            </span>
            <p>
                {"This tool packs, extracts, and compresses XISO images from a source folder or image"}
                {", entirely within your browser."}
            </p>
            <p>{"It is powered by Rust and webassembly. No data leaves your computer during the conversion process."}</p>
            <Callout title={"Disclaimer"} intent={Intent::Warning}>
                {"The developers of this tool do not endorse or promote piracy. This tool is intended for use with
                software you have legal right to use and repack."}
            </Callout>
            <H2>{"What are XISO files, and how are they different from ISOs?"}</H2>
            <p>{"An ISO is a simple disc image file. ISOs make no distinction as to what is inside the disc image."}</p>
            <p>{"In contrast, an XISO file contains data that is of the Xbox DVD Filesystem, or XDVDFS, and thus can
                be read by the Xbox directly"}
            </p>
            <p>{"An ISO copy of an official Xbox disc is not in and of itself an XISO. An official Xbox disc
                    contains two parts: a standard DVD video, and potentially a demo, and the XDVDFS partition
                    containing software. To convert this to an XISO, everything but the XDVDFS partition must be
                    stripped."}
            </p>
        </>
    }
}
