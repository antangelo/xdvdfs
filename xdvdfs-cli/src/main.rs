use clap::Parser;
use maybe_async::maybe_async;

mod cmd_build_image;
mod cmd_compress;
mod cmd_info;
mod cmd_md5;
mod cmd_pack;
mod cmd_read;
mod cmd_unpack;
mod executor;
mod img;

mod exiso_compat;

#[derive(Parser)]
#[command(
    author,
    version,
    about,
    long_about = None,
    arg_required_else_help = true,
    multicall = true,
    allow_external_subcommands = true,
)]
enum TopLevelCommand {
    #[command(name = "xdvdfs", subcommand)]
    Core(Cmd),
    ExtractXiso(exiso_compat::EXCommand),

    // Default any other multicall command to `xdvdfs`
    // external_subcommand must parse into something, even if we don't use it,
    // which necessitates the `dead_code` allow.
    #[command(external_subcommand)]
    #[allow(dead_code)]
    External(Vec<String>),
}

#[derive(Parser)]
#[command(author, version, about, long_about = None, arg_required_else_help = true)]
enum Cmd {
    Ls(cmd_read::LsArgs),
    Tree(cmd_read::TreeArgs),
    Md5(cmd_md5::Md5Args),
    Checksum(cmd_read::ChecksumArgs),
    Info(cmd_info::InfoArgs),
    CopyOut(cmd_unpack::CopyOutArgs),
    Unpack(cmd_unpack::UnpackArgs),
    Pack(cmd_pack::PackArgs),
    BuildImage(cmd_build_image::BuildImageArgs),
    ImageSpec(cmd_build_image::ImageSpecArgs),
    Compress(cmd_compress::CompressArgs),

    #[command(hide = true)]
    ExtractXiso(exiso_compat::EXCommand),
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
        CopyOut(args) => cmd_unpack::cmd_copyout(args).await,
        Unpack(args) => cmd_unpack::cmd_unpack(args).await,
        Pack(args) => cmd_pack::cmd_pack(args).await,
        BuildImage(args) => cmd_build_image::cmd_build_image(args).await,
        ImageSpec(args) => cmd_build_image::cmd_image_spec(args).await,
        Compress(args) => cmd_compress::cmd_compress(args).await,
        ExtractXiso(_) => unreachable!("should be handled before entering async context"),
    }
}

fn run_xdvdfs_program(cmd: &Cmd) {
    if let Cmd::ExtractXiso(exiso_cmd) = cmd {
        exiso_compat::run_exiso_program(exiso_cmd);
        return;
    }

    let res = executor::run_with_executor!(run_command, &cmd);
    if let Err(err) = res {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn main() {
    env_logger::init();

    let tlc = TopLevelCommand::parse();
    match &tlc {
        TopLevelCommand::Core(cli) => run_xdvdfs_program(cli),
        TopLevelCommand::ExtractXiso(cmd) => exiso_compat::run_exiso_program(cmd),
        TopLevelCommand::External(_) => run_xdvdfs_program(&Cmd::parse()),
    }
}
