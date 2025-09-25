use std::{
    fmt::Display,
    fs::Metadata,
    path::{Path, PathBuf},
};

use crate::daemonize::Daemonize;
#[cfg(all(unix, feature = "fuse"))]
use crate::fsproto::fuse::FuseFSMounter;
use crate::fsproto::nfs::NFSMounter;
use crate::fsproto::FSMounter;
use crate::img_fs::ImageFilesystem;
use crate::overlay_fs::OverlayFSBuilder;
use anyhow::bail;
use clap::Parser;
use tokio::runtime::Runtime;

#[derive(Clone, Default, clap::ValueEnum)]
enum Backend {
    #[cfg(all(unix, feature = "fuse"))]
    #[default]
    Fuse,

    #[cfg_attr(not(all(unix, feature = "fuse")), default)]
    Nfs,
}

impl Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(all(unix, feature = "fuse"))]
            Self::Fuse => f.write_str("fuse"),
            Self::Nfs => f.write_str("nfs"),
        }
    }
}

#[derive(Parser)]
#[command(
    author,
    version,
    about,
    long_about = None,
    arg_required_else_help = true,
)]
pub struct MountArgs {
    #[arg(required = true, index = 1)]
    source: PathBuf,

    #[arg(index = 2)]
    mount_point: Option<PathBuf>,

    #[arg(short = 'o')]
    options: Vec<String>,

    #[arg(short = 'b', default_value_t=Backend::default())]
    backend: Backend,
}

fn mount_image_file<FSM: FSMounter>(
    mount_point: Option<&Path>,
    src: &Path,
    src_metadata: &Metadata,
    options: &[String],
) -> anyhow::Result<()> {
    let mut fsm = FSM::default();
    let top_level_opts = fsm.process_args(mount_point, src, options)?;

    // Safety: Only one thread is currently active
    let dm = top_level_opts
        .fork
        .then(|| unsafe { Daemonize::fork() })
        .transpose()?;

    let rt = Runtime::new()?;
    let fs = rt.block_on(ImageFilesystem::new(src, src_metadata))?;

    if let Some(dm) = dm {
        dm.finish()?;
    }

    fsm.mount(fs, &rt, mount_point)
}

fn mount_pack_overlay<FSM: FSMounter>(
    mount_point: Option<&Path>,
    src: &Path,
    options: &[String],
) -> anyhow::Result<()> {
    let mut fsm = FSM::default();
    let top_level_opts = fsm.process_args(mount_point, src, options)?;

    // Safety: Only one thread is currently active
    let dm = top_level_opts
        .fork
        .then(|| unsafe { Daemonize::fork() })
        .transpose()?;

    let rt = Runtime::new()?;
    let fs = OverlayFSBuilder::new(src)
        .with_provider(crate::img_fs::ImageFilesystemProvider)
        .with_provider(crate::overlay_fs::truncatefs::ImageTruncateFSFileProvider)
        .with_provider(crate::overlay_fs::packfs::PackOverlayProvider)
        .build()?;

    if let Some(dm) = dm {
        dm.finish()?;
    }

    fsm.mount(fs, &rt, mount_point)
}

pub fn run_mount_program(args: &MountArgs) -> anyhow::Result<()> {
    if let Some(mount_point) = &args.mount_point {
        let mount_point_metadata = std::fs::metadata(mount_point)?;
        if !mount_point_metadata.is_dir() {
            bail!("Mount point must be a directory");
        }
    }
    let mount_point = args.mount_point.as_deref();

    // Follow symlinks if possible
    let source = std::fs::canonicalize(&args.source);
    let source = match source {
        Ok(source) => source,
        Err(_) => std::path::absolute(&args.source)?,
    };

    let src_metadata = std::fs::metadata(&source)?;
    if src_metadata.is_file() {
        match args.backend {
            #[cfg(all(unix, feature = "fuse"))]
            Backend::Fuse => mount_image_file::<FuseFSMounter>(
                mount_point,
                &source,
                &src_metadata,
                &args.options,
            ),
            Backend::Nfs => {
                mount_image_file::<NFSMounter>(mount_point, &source, &src_metadata, &args.options)
            }
        }
    } else if src_metadata.is_dir() {
        match args.backend {
            #[cfg(all(unix, feature = "fuse"))]
            Backend::Fuse => {
                mount_pack_overlay::<FuseFSMounter>(mount_point, &source, &args.options)
            }
            Backend::Nfs => mount_pack_overlay::<NFSMounter>(mount_point, &source, &args.options),
        }
    } else {
        bail!("Unsupported source file type")
    }
}

pub fn mount_main() -> anyhow::Result<()> {
    let args = MountArgs::parse();
    run_mount_program(&args)
}
