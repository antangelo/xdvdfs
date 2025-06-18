use std::{
    fmt::Display,
    fs::Metadata,
    path::{Path, PathBuf},
};

use anyhow::bail;
use clap::Parser;
use fsproto::{fuse::FuseFilesystem, nfs::NFSFilesystem};
use fuser::MountOption;
use img_fs::ImageFilesystem;
use nfsserve::tcp::{NFSTcp, NFSTcpListener};
use tokio::runtime::Runtime;

mod daemonize;
mod fsproto;
mod img_fs;
mod inode;

#[derive(Clone, Default, clap::ValueEnum)]
enum Backend {
    #[default]
    Fuse,
    Nfs,
}

impl Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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

    #[arg(short = 'b', default_value_t=Backend::Fuse)]
    backend: Backend,
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

struct NFSMountArgs {
    port: u16,
}

fn convert_nfs_mount_opts(opts: &[String]) -> anyhow::Result<NFSMountArgs> {
    let mut args = NFSMountArgs { port: 11111 };

    for opt in opts {
        match opt {
            x if x.starts_with("port=") => {
                args.port = x[5..].parse()?;
            }
            x => bail!("Unsupported mount option {x}"),
        }
    }

    Ok(args)
}

fn mount_image_file_fuse(
    mount_point: Option<&Path>,
    src: &Path,
    metadata: Metadata,
    opts: &[String],
) -> anyhow::Result<()> {
    let Some(mount_point) = mount_point else {
        bail!("Mount point must be specified for FUSE mounting");
    };

    let (top_level_opts, mount_opts) = convert_fuse_mount_opts(src, opts)?;

    // Safety: Only one thread is currently active
    let dm = top_level_opts
        .fork
        .then(|| unsafe { daemonize::Daemonize::fork() })
        .transpose()?;

    let rt = Runtime::new()?;
    let fs = rt.block_on(ImageFilesystem::new(src, metadata))?;
    let fs = FuseFilesystem::new(&fs, rt);
    if let Some(dm) = dm {
        dm.finish()?;
    }
    fuser::mount2(fs, mount_point, &mount_opts)?;
    Ok(())
}

fn mount_image_file_nfs(
    mount_point: Option<&Path>,
    src: &Path,
    metadata: Metadata,
    opts: &[String],
) -> anyhow::Result<()> {
    let args = convert_nfs_mount_opts(opts)?;

    let mount_point_string = match mount_point {
        Some(mount_point) => mount_point.display().to_string(),
        None => "<mount point>".to_string(),
    };

    let rt = Runtime::new()?;
    rt.block_on(async move {
        let fs = ImageFilesystem::new(src, metadata).await?;
        let nfs = NFSFilesystem::new(fs);
        let listener = NFSTcpListener::bind(&format!("127.0.0.1:{}", args.port), nfs).await?;

        println!("NFS server listening on port {}", args.port);

        // FIXME: Support mount hints for other operating systems
        println!("Mount with (may require root):");
        println!("mount -t nfs -o user,noacl,nolock,vers=3,tcp,wsize=1048576,rsize=131072,actimeo=120,port={},mountport={} localhost:/ {mount_point_string}", args.port, args.port);

        listener.handle_forever().await?;

        Ok(())
    })
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
            Backend::Fuse => {
                mount_image_file_fuse(mount_point, &source, src_metadata, &args.options)
            }
            Backend::Nfs => mount_image_file_nfs(mount_point, &source, src_metadata, &args.options),
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
