use std::path::{Path, PathBuf};

use anyhow::bail;
use chrono::Utc;
use clap::Args;
use maybe_async::maybe_async;
use xdvdfs::util::FileTime;
use xdvdfs::write::{self, img::ProgressInfo};

use crate::img::{absolute_path, with_extension};

#[derive(Args)]
#[command(about = "Pack an image from a given directory or source ISO image")]
pub struct PackArgs {
    #[arg(help = "Path to source directory or ISO image")]
    pub source_path: String,

    #[arg(help = "Path to output image")]
    pub image_path: Option<String>,

    #[arg(
        long,
        short = 'T',
        help = "Use current time as creation time",
        group = "timestamp_source"
    )]
    pub timestamp_now: bool,

    #[arg(
        long,
        short = 't',
        value_name = "TIMESTAMP",
        help = "Set a custom creation time (Windows FileTime format)",
        group = "timestamp_source"
    )]
    pub timestamp: Option<u64>
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

    let time = if let Some(ts) = args.timestamp {
        FileTime::from_windows_timestamp(ts)
    } else if args.timestamp_now {
        let now = Utc::now();
        FileTime::from_unix_timestamp(now.timestamp())
    } else {
        FileTime::default()
    };

    cmd_pack_path(&source_path, &image_path, time).await
}

#[maybe_async]
pub async fn cmd_pack_path(source_path: &Path, image_path: &Path, file_time: FileTime) -> Result<(), anyhow::Error> {
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

    if is_dir {
        let mut fs = write::fs::StdFilesystem::create(source_path);
        write::img::create_xdvdfs_image_with_filetime(&mut fs, &mut image, file_time, progress_callback).await?;
    } else if meta.is_file() {
        let source = crate::img::open_image_raw(source_path).await?;
        let mut fs = write::fs::XDVDFSFilesystem::<_, _, write::fs::StdIOCopier<_, _>>::new(source)
            .await
            .ok_or(anyhow::anyhow!("Failed to create XDVDFS filesystem"))?;
        write::img::create_xdvdfs_image_with_filetime(&mut fs, &mut image, file_time, progress_callback).await?;
    } else {
        return Err(anyhow::anyhow!("Symlink image sources are not supported"));
    }

    Ok(())
}
