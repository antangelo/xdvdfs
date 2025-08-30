use std::path::{Path, PathBuf};

use anyhow::bail;
use clap::Args;
use maybe_async::maybe_async;
use xdvdfs::{blockdev::StdIOCopier, write::{
    fs::{StdFilesystem, XDVDFSFilesystem},
    img::create_xdvdfs_image,
}};

use crate::{
    img::{absolute_path, with_extension},
    progress::StdIoProgressReporter,
};

#[derive(Args)]
#[command(about = "Pack an image from a given directory or source ISO image")]
pub struct PackArgs {
    #[arg(help = "Path to source directory or ISO image")]
    pub source_path: String,

    #[arg(help = "Path to output image")]
    pub image_path: Option<String>,

    #[arg(help = "test", long, short = "t")]
    pub test: Option<Option<String>>,
}

fn get_default_image_path(source_path: &Path, is_dir: bool) -> Result<PathBuf, anyhow::Error> {
    let source_file_name = source_path
        .file_name()
        .ok_or(anyhow::anyhow!("Failed to get file name from source path"))?;
    let output = with_extension(Path::new(source_file_name), "iso", is_dir);

    if output.exists() && absolute_path(&output)? == source_path {
        return Ok(with_extension(
            Path::new(source_file_name),
            "xiso.iso",
            is_dir,
        ));
    }

    Ok(output)
}

#[maybe_async]
pub async fn cmd_pack(args: &PackArgs) -> Result<(), anyhow::Error> {
    let source_path = absolute_path(Path::new(&args.source_path))?;
    let meta = std::fs::metadata(&source_path)?;
    let is_dir = meta.is_dir();

    let image_path = args
        .image_path
        .as_ref()
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| get_default_image_path(&source_path, is_dir))?;

    cmd_pack_path(&source_path, &image_path).await
}

#[maybe_async]
pub async fn cmd_pack_path(source_path: &Path, image_path: &Path) -> Result<(), anyhow::Error> {
    let meta = std::fs::metadata(source_path)?;
    let is_dir = meta.is_dir();

    if image_path.exists() && absolute_path(image_path)? == source_path {
        bail!("Source and destination paths are the same");
    }

    if absolute_path(image_path)?.starts_with(source_path) {
        bail!("Destination path is contained by source path");
    }

    let image = std::fs::File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(image_path)?;
    let mut image = std::io::BufWriter::with_capacity(1024 * 1024, image);

    let progress_visitor = StdIoProgressReporter::new(source_path, is_dir);
    if is_dir {
        let mut fs = StdFilesystem::create(source_path);
        create_xdvdfs_image(&mut fs, &mut image, progress_visitor).await?;
    } else if meta.is_file() {
        let source = crate::img::open_image_raw(source_path).await?;
        let mut fs = XDVDFSFilesystem::<_, _, StdIOCopier<_, _>>::new(source)
            .await
            .ok_or(anyhow::anyhow!("Failed to create XDVDFS filesystem"))?;
        create_xdvdfs_image(&mut fs, &mut image, progress_visitor).await?;
    } else {
        bail!("Symlink image sources are not supported");
    }

    Ok(())
}
