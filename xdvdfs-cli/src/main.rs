use std::{path::PathBuf, str::FromStr};

use clap::{Parser, Subcommand};

mod cmd_md5;
mod cmd_pack;
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
    #[group(id = "read")]
    Ls {
        #[arg(help = "Path to XISO image")]
        image_path: String,

        #[arg(default_value = "/", help = "Directory to list")]
        path: String,
    },
    #[command(about = "List all files in an image, recursively")]
    #[group(id = "read")]
    Tree {
        #[arg(help = "Path to XISO image")]
        image_path: String,
    },
    #[command(about = "Show MD5 checksums for files in an image")]
    #[group(id = "read")]
    Md5 {
        #[arg(help = "Path to XISO image")]
        image_path: String,

        #[arg(help = "Target file within image")]
        path: Option<String>,
    },
    #[command(about = "Unpack an entire image to a directory")]
    #[group(id = "read")]
    Unpack {
        #[arg(help = "Path to XISO image")]
        image_path: String,

        #[arg(help = "Output directory")]
        path: Option<String>,
    },
    #[command(about = "Pack an image from a given directory")]
    #[group(id = "write")]
    Pack {
        #[arg(help = "Path to source directory")]
        source_path: String,

        #[arg(help = "Path to output image")]
        image_path: Option<String>,
    },
}

fn run_command(cmd: &Cmd) {
    use Cmd::*;
    let res = match cmd {
        Ls { image_path, path } => cmd_read::cmd_ls(image_path, path),
        Tree { image_path } => cmd_read::cmd_tree(image_path),
        Md5 { image_path, path } => cmd_md5::cmd_md5(image_path, path.clone().as_deref()),
        Unpack { image_path, path } => {
            let path = match path {
                Some(path) => PathBuf::from_str(path).unwrap(),
                None => {
                    let os_path = PathBuf::from_str(image_path).unwrap();
                    PathBuf::from(os_path.file_name().unwrap()).with_extension("")
                }
            };

            cmd_read::cmd_unpack(image_path, &path)
        }
        Pack {
            source_path,
            image_path,
        } => cmd_pack::cmd_pack(&source_path, &image_path),
    };

    res.unwrap();
}

fn main() {
    let cli = Args::parse();
    if let Some(cmd) = cli.command {
        run_command(&cmd);
    }
}
