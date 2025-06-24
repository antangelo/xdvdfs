use std::{
    fmt::Display,
    fs::Metadata,
    path::{Path, PathBuf},
};

use anyhow::bail;
use clap::Parser;
use fsproto::FSMounter;
use img_fs::ImageFilesystem;
use tokio::runtime::Runtime;

pub mod fsproto;

mod daemonize;
mod img_fs;
mod inode;

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
        .then(|| unsafe { daemonize::Daemonize::fork() })
        .transpose()?;

    let rt = Runtime::new()?;
    let fs = rt.block_on(ImageFilesystem::new(src, src_metadata))?;

    if let Some(dm) = dm {
        dm.finish()?;
    }

    fsm.mount(fs, &rt, mount_point)
}

fn mount_pack_overlay(
    _mount_point: Option<&Path>,
    _src: &Path,
    _opts: &[String],
) -> anyhow::Result<()> {
    unimplemented!("Overlay filesystem is not yet implemented!")
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
            Backend::Fuse => mount_image_file::<fsproto::fuse::FuseFSMounter>(
                mount_point,
                &source,
                &src_metadata,
                &args.options,
            ),
            Backend::Nfs => mount_image_file::<fsproto::nfs::NFSMounter>(
                mount_point,
                &source,
                &src_metadata,
                &args.options,
            ),
        }
    } else if src_metadata.is_dir() {
        mount_pack_overlay(mount_point, &source, &args.options)
    } else {
        bail!("Unsupported source file type")
    }
}

fn main() {
    env_logger::init();

    let args = MountArgs::parse();
    let res = run_mount_program(&args);
    if let Err(err) = res {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
