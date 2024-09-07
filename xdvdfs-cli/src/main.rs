use clap::{Parser, Subcommand};
use maybe_async::maybe_async;

mod cmd_build_image;
mod cmd_compress;
mod cmd_info;
mod cmd_md5;
mod cmd_pack;
mod cmd_read;
mod cmd_unpack;
mod img;

#[derive(Parser)]
#[command(author, version, about, long_about = None, arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    Ls(cmd_read::LsArgs),
    Tree(cmd_read::TreeArgs),
    Md5(cmd_md5::Md5Args),
    Checksum(cmd_read::ChecksumArgs),
    Info(cmd_info::InfoArgs),
    Unpack(cmd_unpack::UnpackArgs),
    Pack(cmd_pack::PackArgs),
    BuildImage(cmd_build_image::BuildImageArgs),
    ImageSpec(cmd_build_image::ImageSpecArgs),
    Compress(cmd_compress::CompressArgs),
}

#[maybe_async]
async fn run_command(cmd: &Cmd) -> Result<(), anyhow::Error> {
    use Cmd::*;
    match cmd {
        Ls(args) => cmd_read::cmd_ls(args).await,
        Tree(args) => cmd_read::cmd_tree(args).await,
        Md5(args) => cmd_md5::cmd_md5(args).await,
        Checksum(args) => cmd_read::cmd_checksum(args).await,
        Info(args) => cmd_info::cmd_info(args).await,
        Unpack(args) => cmd_unpack::cmd_unpack(args).await,
        Pack(args) => cmd_pack::cmd_pack(args).await,
        BuildImage(args) => cmd_build_image::cmd_build_image(args).await,
        ImageSpec(args) => cmd_build_image::cmd_image_spec(args).await,
        Compress(args) => cmd_compress::cmd_compress(args).await,
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

    let cli = Cli::parse();
    if let Some(cmd) = cli.command {
        let res = run_program(&cmd);
        if let Err(err) = res {
            eprintln!("error: {}", err);
            std::process::exit(1);
        }
    }
}
