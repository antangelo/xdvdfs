use std::{fs::File, path::PathBuf};

use clap::Args;
use maybe_async::maybe_async;

use crate::img::absolute_path;

#[derive(Args)]
#[command(
    about = "Compress a raw ISO (2048-byte sectors) to CCI (LZ4). Split parts are written as stem.1.cci, stem.2.cci."
)]
pub struct CciEncodeArgs {
    #[arg(help = "Source ISO path")]
    pub source: String,

    #[arg(help = "Output path (e.g. game.cci). Multiple parts rename to game.1.cci, game.2.cci, …")]
    pub dest: String,

    #[arg(
        long,
        help = "Split when the estimated part size would exceed this many bytes (0 = single file). Typical: FATX-safe ~4_290_735_312"
    )]
    pub split: Option<u64>,
}

#[derive(Args)]
#[command(
    about = "Decompress CCI to a raw ISO. Open any slice (e.g. game.1.cci) to read the full split set."
)]
pub struct CciDecodeArgs {
    #[arg(help = "Source .cci or first slice (name.1.cci)")]
    pub source: String,

    #[arg(help = "Output ISO path")]
    pub dest: String,
}

#[maybe_async]
pub async fn cmd_cci_encode(args: &CciEncodeArgs) -> Result<(), anyhow::Error> {
    let source_path = PathBuf::from(&args.source);
    let dest_path = PathBuf::from(&args.dest);

    if !source_path.is_file() {
        anyhow::bail!("Source {:?} is not a file", source_path);
    }

    if dest_path.exists() && absolute_path(&dest_path)? == absolute_path(&source_path)? {
        anyhow::bail!("Source and destination paths are the same");
    }

    if dest_path.starts_with(&source_path) {
        anyhow::bail!("Destination path is contained by source path");
    }

    let len = source_path.metadata()?.len();
    let split = args.split.unwrap_or(0);
    let mut input = File::open(&source_path)?;
    xdvdfs_cci::iso_to_cci(&mut input, len, &dest_path, split)?;
    Ok(())
}

#[maybe_async]
pub async fn cmd_cci_decode(args: &CciDecodeArgs) -> Result<(), anyhow::Error> {
    let source_path = PathBuf::from(&args.source);
    let dest_path = PathBuf::from(&args.dest);

    if !source_path.is_file() {
        anyhow::bail!("Source {:?} is not a file", source_path);
    }

    if dest_path.exists() && absolute_path(&dest_path)? == absolute_path(&source_path)? {
        anyhow::bail!("Source and destination paths are the same");
    }

    let mut out = File::create(&dest_path)?;
    xdvdfs_cci::cci_to_iso(&source_path, &mut out)?;
    Ok(())
}
