use std::path::{Path, PathBuf};
use chrono::Utc;
use clap::Args;
use maybe_async::maybe_async;

use xdvdfs::{
    blockdev,
    write::{self, img::ProgressInfo},
};
use xdvdfs::util::FileTime;
use crate::img::{absolute_path, open_image_raw, with_extension};

#[derive(Args)]
#[command(about = "Pack and compress an image from a given directory or source ISO image")]
pub struct CompressArgs {
    #[arg(help = "Path to source directory or ISO image")]
    source_path: String,

    #[arg(help = "Path to output image")]
    image_path: Option<String>,

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

struct SplitStdFs;

type BufFile = std::io::BufWriter<std::fs::File>;
type BufFileSectorLinearFs<'a> = write::fs::SectorLinearBlockFilesystem<
    &'a mut write::fs::XDVDFSFilesystem<
        blockdev::OffsetWrapper<std::io::BufReader<std::fs::File>>,
        [u8],
        write::fs::DefaultCopier<blockdev::OffsetWrapper<std::io::BufReader<std::fs::File>>, [u8]>,
    >,
>;

#[maybe_async]
impl ciso::split::SplitFilesystem<std::io::Error, BufFile> for SplitStdFs {
    async fn create_file(&mut self, name: &std::ffi::OsStr) -> Result<BufFile, std::io::Error> {
        let file = std::fs::File::create(name)?;
        let bf: BufFile = std::io::BufWriter::new(file);
        Ok(bf)
    }

    async fn close(&mut self, _: BufFile) {}
}

fn get_default_image_path(source_path: &Path, is_dir: bool) -> Option<PathBuf> {
    let source_file_name = source_path.file_name()?;
    let output = with_extension(Path::new(source_file_name), "cso", is_dir);

    Some(output)
}

#[maybe_async]
pub async fn cmd_compress(args: &CompressArgs) -> Result<(), anyhow::Error> {
    let source_path = PathBuf::from(&args.source_path);
    let meta = std::fs::metadata(&source_path)?;
    let is_dir = meta.is_dir();

    let image_path = args
        .image_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| get_default_image_path(&source_path, is_dir).unwrap());

    // This is unlikely to happen, since compressed input is unsupported
    // and this will fail anyway, but we check to avoid truncating the input accidentally
    if image_path.exists() && absolute_path(&image_path)? == source_path {
        return Err(anyhow::anyhow!("Source and destination paths are the same"));
    }

    if image_path.starts_with(&source_path) {
        return Err(anyhow::anyhow!(
            "Destination path is contained by source path"
        ));
    }

    let mut output = ciso::split::SplitOutput::new(SplitStdFs, image_path);

    let progress_callback = |pi| match pi {
        ProgressInfo::DirAdded(path, sector) => {
            println!("Added dir: {path:?} at sector {sector}");
        }
        ProgressInfo::FileAdded(path, sector) => {
            println!("Added file: {path:?} at sector {sector}");
        }
        _ => {}
    };

    let time = if let Some(ts) = args.timestamp {
        FileTime::from_windows_timestamp(ts)
    } else if args.timestamp_now {
        let now = Utc::now();
        FileTime::from_unix_timestamp(now.timestamp())
    } else {
        FileTime::default()
    };

    let mut total_sectors = 0;
    let mut sectors_finished = 0;
    let progress_callback_compression = |pi| match pi {
        ciso::write::ProgressInfo::SectorCount(c) => total_sectors = c,
        ciso::write::ProgressInfo::SectorFinished => {
            sectors_finished += 1;
            print!("\rCompressing sectors ({sectors_finished}/{total_sectors})");
        }
        ciso::write::ProgressInfo::Finished => println!(),
        _ => {}
    };

    if is_dir {
        let mut fs = write::fs::StdFilesystem::create(&source_path);
        let mut slbd = write::fs::SectorLinearBlockDevice::default();
        let mut slbfs = write::fs::SectorLinearBlockFilesystem::new(&mut fs);

        write::img::create_xdvdfs_image_with_filetime(
            &mut slbfs, 
            &mut slbd, 
            time, 
            progress_callback
        ).await?;

        let mut input = write::fs::SectorLinearImage::new(&slbd, &mut slbfs);
        ciso::write::write_ciso_image(&mut input, &mut output, progress_callback_compression)
            .await?;
    } else if meta.is_file() {
        let source = open_image_raw(&source_path).await?;
        let mut fs = write::fs::XDVDFSFilesystem::new(source)
            .await
            .ok_or(anyhow::anyhow!("Failed to create XDVDFS filesystem"))?;

        let mut slbd = write::fs::SectorLinearBlockDevice::default();
        let mut slbfs: BufFileSectorLinearFs = write::fs::SectorLinearBlockFilesystem::new(&mut fs);

        write::img::create_xdvdfs_image_with_filetime(
            &mut slbfs, 
            &mut slbd, 
            time, 
            progress_callback
        ).await?;

        let mut input = write::fs::SectorLinearImage::new(&slbd, &mut slbfs);
        ciso::write::write_ciso_image(&mut input, &mut output, progress_callback_compression)
            .await?;
    } else {
        return Err(anyhow::anyhow!("Symlink image sources are not supported"));
    }

    Ok(())
}
