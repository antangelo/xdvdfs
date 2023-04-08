use clap::{Parser, Subcommand};

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
}

fn run_command(cmd: &Cmd) {
    use Cmd::*;
    let res = match cmd {
        Ls { image_path, path } => cmd_read::cmd_ls(image_path, path),
        Tree { image_path } => cmd_read::cmd_tree(image_path),
        Md5 { image_path, path } => cmd_read::cmd_md5(image_path, path.clone().as_deref()),
    };

    res.unwrap();
}

fn main() {
    let cli = Args::parse();
    if let Some(cmd) = cli.command {
        run_command(&cmd);
    }
}
