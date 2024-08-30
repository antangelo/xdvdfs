use std::path::{Path, PathBuf};

use clap::Args;
use maybe_async::maybe_async;
use xdvdfs::write::{self, img::ProgressInfo};

#[derive(Args)]
#[command(about = "Pack an image from a given directory or source ISO image")]
pub struct PackArgs {
    #[arg(help = "Path to source directory or ISO image")]
    source_path: String,

    #[arg(help = "Path to output image")]
    image_path: Option<String>,
}

fn get_default_image_path(source_path: &Path) -> Result<PathBuf, anyhow::Error> {
    let source_file_name = source_path
        .file_name()
        .ok_or(anyhow::anyhow!("Failed to get file name from source path"))?;
    let output = PathBuf::from(source_file_name).with_extension("iso");

    if output.exists() && output.canonicalize()? == source_path {
        return Ok(PathBuf::from(source_file_name).with_extension("xiso.iso"));
    }

    Ok(output)
}

#[maybe_async]
pub async fn cmd_pack(args: &PackArgs) -> Result<(), anyhow::Error> {
    let source_path = PathBuf::from(&args.source_path).canonicalize()?;

    let image_path = args
        .image_path
        .as_ref()
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| get_default_image_path(&source_path))?;

    if image_path.exists() && image_path.canonicalize()? == source_path {
        return Err(anyhow::anyhow!("Source and destination paths are the same"));
    }

    let image = std::fs::File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(image_path)?;
    let mut image = std::io::BufWriter::with_capacity(1024 * 1024, image);

    let mut file_count: usize = 0;
    let mut progress_count: usize = 0;
    let progress_callback = |pi| match pi {
        ProgressInfo::FileCount(count) => file_count += count,
        ProgressInfo::DirCount(count) => file_count += count,
        ProgressInfo::DirAdded(path, sector) => {
            progress_count += 1;
            println!("[{progress_count}/{file_count}] Added dir: {path} at sector {sector}");
        }
        ProgressInfo::FileAdded(path, sector) => {
            progress_count += 1;
            println!("[{progress_count}/{file_count}] Added file: {path} at sector {sector}");
        }
        _ => {}
    };

    let meta = std::fs::metadata(&source_path)?;
    if meta.is_dir() {
        let mut fs = write::fs::StdFilesystem::create(&source_path);
        write::img::create_xdvdfs_image(&mut fs, &mut image, progress_callback).await?;
    } else if meta.is_file() {
        let source = crate::img::open_image_raw(&source_path).await?;
        let mut fs = write::fs::XDVDFSFilesystem::new(source)
            .await
            .ok_or(anyhow::anyhow!("Failed to create XDVDFS filesystem"))?;
        write::img::create_xdvdfs_image(&mut fs, &mut image, progress_callback).await?;
    } else {
        return Err(anyhow::anyhow!("Symlink image sources are not supported"));
    }

    Ok(())
}
