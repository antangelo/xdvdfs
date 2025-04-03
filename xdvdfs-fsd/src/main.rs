use std::{
    fs::Metadata,
    path::{Path, PathBuf},
};

use anyhow::bail;
use clap::Parser;
use fuser::MountOption;
use img_fs::FuseFilesystem;
use tokio::runtime::Runtime;

mod daemonize;
mod img_fs;

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

    #[arg(required = true, index = 2)]
    mount_point: PathBuf,

    #[arg(short = 'o')]
    options: Vec<String>,
}

#[derive(Copy, Clone)]
pub struct TopLevelOptions {
    fork: bool,
}

fn convert_fuse_mount_opts(
    src: &Path,
    opts: &[String],
) -> anyhow::Result<(TopLevelOptions, Vec<MountOption>)> {
    let mut mount_opts = Vec::new();
    mount_opts.reserve_exact(opts.len());

    let mut has_fsname = false;
    let mut has_subtype = false;
    let mut tlo = TopLevelOptions { fork: true };

    for opt in opts {
        for opt in opt.split(",") {
            let opt = match opt {
                "auto_unmount" => MountOption::AutoUnmount,
                "allow_other" => MountOption::AllowOther,
                "allow_root" => MountOption::AllowRoot,
                "default_permissions" => MountOption::DefaultPermissions,
                "suid" => MountOption::Suid,
                "nosuid" => MountOption::NoSuid,
                "ro" => MountOption::RO,
                "rw" => MountOption::RW,
                "exec" => MountOption::Exec,
                "noexec" => MountOption::NoExec,
                "dev" => MountOption::Dev,
                "nodev" => MountOption::NoDev,
                x if x.starts_with("fsname=") => {
                    has_fsname = true;
                    MountOption::FSName(x[7..].into())
                }
                x if x.starts_with("subtype=") => {
                    has_subtype = true;
                    MountOption::Subtype(x[8..].into())
                }
                "fork" => {
                    tlo.fork = true;
                    continue;
                }
                "nofork" => {
                    tlo.fork = false;
                    continue;
                }
                x => bail!("Unsupported mount option {x}"),
            };

            mount_opts.push(opt);
        }
    }

    if !has_fsname {
        mount_opts.push(MountOption::FSName(src.to_string_lossy().to_string()));
    }

    if !has_subtype {
        mount_opts.push(MountOption::Subtype("xdvdfs".to_string()));
    }

    Ok((tlo, mount_opts))
}

fn mount_image_file(
    mount_point: &Path,
    src: &Path,
    metadata: Metadata,
    opts: &[String],
) -> anyhow::Result<()> {
    let (top_level_opts, mount_opts) = convert_fuse_mount_opts(src, opts)?;

    // Safety: Only one thread is currently active
    let dm = top_level_opts
        .fork
        .then(|| unsafe { daemonize::Daemonize::fork() })
        .transpose()?;

    let rt = Runtime::new()?;
    let fs = FuseFilesystem::new(src, metadata, rt)?;
    if let Some(dm) = dm {
        dm.finish()?;
    }
    fuser::mount2(fs, mount_point, &mount_opts)?;
    Ok(())
}

fn mount_pack_overlay(_mount_point: &Path, _src: &Path, _opts: &[String]) -> anyhow::Result<()> {
    unimplemented!("Overlay filesystem is not yet implemented!")
}

pub fn run_mount_program(args: &MountArgs) -> anyhow::Result<()> {
    let mount_point_metadata = std::fs::metadata(&args.mount_point)?;
    if !mount_point_metadata.is_dir() {
        bail!("Mount point must be a directory");
    }

    // Follow symlinks if possible
    let source = std::fs::canonicalize(&args.source);
    let source = match source {
        Ok(source) => source,
        Err(_) => std::path::absolute(&args.source)?,
    };
    let src_metadata = std::fs::metadata(&source)?;

    if src_metadata.is_file() {
        mount_image_file(&args.mount_point, &source, src_metadata, &args.options)
    } else if src_metadata.is_dir() {
        mount_pack_overlay(&args.mount_point, &source, &args.options)
    } else {
        bail!("Unsupported source file type");
    }
}

fn main() {
    env_logger::init();

    let args = MountArgs::parse();
    let res = run_mount_program(&args);
    if let Err(err) = res {
        eprintln!("error: {}", err);
        std::process::exit(1);
    }
}
