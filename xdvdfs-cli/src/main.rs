use clap::{Parser, Subcommand};

mod cmd_info;
mod cmd_md5;
mod cmd_pack;
mod cmd_patch;
mod cmd_read;

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
    #[command(about = "Patch XBE files in an image to run on any media type")]
    Patch {
        #[arg(help = "Path to XISO image")]
        image_path: String,
    },
    #[command(about = "Unpack an entire image to a directory")]
    Unpack {
        #[arg(help = "Path to XISO image")]
        image_path: String,

        #[arg(help = "Output directory")]
        path: Option<String>,
    },
    #[command(about = "Pack an image from a given directory")]
    Pack {
        #[arg(help = "Path to source directory")]
        source_path: String,

        #[arg(help = "Path to output image")]
        image_path: Option<String>,
    },
}

fn run_command(cmd: &Cmd) -> Result<(), String> {
    use Cmd::*;
    match cmd {
        Ls { image_path, path } => cmd_read::cmd_ls(image_path, path),
        Tree { image_path } => cmd_read::cmd_tree(image_path),
        Md5 { image_path, path } => cmd_md5::cmd_md5(image_path, path.clone().as_deref()),
        Info {
            image_path,
            file_entry,
        } => cmd_info::cmd_info(image_path, file_entry.as_ref()),
        Patch { image_path } => cmd_patch::cmd_patch(image_path),
        Unpack { image_path, path } => cmd_read::cmd_unpack(image_path, path),
        Pack {
            source_path,
            image_path,
        } => cmd_pack::cmd_pack(source_path, image_path),
    }
}

fn main() {
    let cli = Args::parse();
    if let Some(cmd) = cli.command {
        let res = run_command(&cmd);
        if let Err(err) = res {
            eprintln!("Error: {}", err);
            std::process::exit(1);
        }
    }
}
