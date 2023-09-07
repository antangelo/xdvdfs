use clap::{Parser, Subcommand};
use maybe_async::maybe_async;

mod cmd_compress;
mod cmd_info;
mod cmd_md5;
mod cmd_pack;
mod cmd_read;
mod img;

#[derive(Parser)]
#[command(author, version, about, long_about = None, arg_required_else_help = true)]
struct Args {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    #[command(about = "List files in an image")]
    Ls {
        #[arg(help = "Path to XISO image")]
        image_path: String,

        #[arg(default_value = "/", help = "Directory to list")]
        path: String,
    },
    #[command(about = "List all files in an image, recursively")]
    Tree {
        #[arg(help = "Path to XISO image")]
        image_path: String,
    },
    #[command(about = "Show MD5 checksums for files in an image")]
    Md5 {
        #[arg(help = "Path to XISO image")]
        image_path: String,

        #[arg(help = "Target file within image")]
        path: Option<String>,
    },
    #[command(about = "Compute deterministic checksum of image contents")]
    Checksum {
        #[arg(help = "Path to XISO image")]
        images: Vec<String>,
    },
    #[command(
        about = "Print information about image metadata",
        long_about = "\
        Print information about image metadata. \
        If a file is specified, prints its directory entry. \
        If no file is specified, prints volume metadata."
    )]
    Info {
        #[arg(help = "Path to XISO image")]
        image_path: String,

        #[arg(help = "Path to file/directory within image")]
        file_entry: Option<String>,
    },
    #[command(about = "Unpack an entire image to a directory")]
    Unpack {
        #[arg(help = "Path to XISO image")]
        image_path: String,

        #[arg(help = "Output directory")]
        path: Option<String>,
    },
    #[command(about = "Pack an image from a given directory or source ISO image")]
    Pack {
        #[arg(help = "Path to source directory or ISO image")]
        source_path: String,

        #[arg(help = "Path to output image")]
        image_path: Option<String>,
    },
    #[command(about = "Pack and compress an image from a given directory or source ISO image")]
    Compress {
        #[arg(help = "Path to source directory or ISO image")]
        source_path: String,

        #[arg(help = "Path to output image")]
        image_path: Option<String>,
    },
}

#[maybe_async]
async fn run_command(cmd: &Cmd) -> Result<(), anyhow::Error> {
    use Cmd::*;
    match cmd {
        Ls { image_path, path } => cmd_read::cmd_ls(image_path, path).await,
        Tree { image_path } => cmd_read::cmd_tree(image_path).await,
        Md5 { image_path, path } => cmd_md5::cmd_md5(image_path, path.clone().as_deref()).await,
        Checksum { images } => cmd_read::cmd_checksum(images).await,
        Info {
            image_path,
            file_entry,
        } => cmd_info::cmd_info(image_path, file_entry.as_ref()).await,
        Unpack { image_path, path } => cmd_read::cmd_unpack(image_path, path).await,
        Pack {
            source_path,
            image_path,
        } => cmd_pack::cmd_pack(source_path, image_path).await,
        Compress {
            source_path,
            image_path,
        } => cmd_compress::cmd_compress(source_path, image_path).await,
    }
}

#[cfg(feature = "sync")]
fn run_program(cmd: &Cmd) -> anyhow::Result<()> {
    run_command(&cmd)
}

#[cfg(not(feature = "sync"))]
fn run_program(cmd: &Cmd) -> anyhow::Result<()> {
    futures::executor::block_on(run_command(cmd))
}

fn main() {
    env_logger::init();

    let cli = Args::parse();
    if let Some(cmd) = cli.command {
        let res = run_program(&cmd);
        if let Err(err) = res {
            eprintln!("Error: {}", err);
            std::process::exit(1);
        }
    }
}
