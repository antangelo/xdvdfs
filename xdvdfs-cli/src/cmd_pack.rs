use std::path::{Path, PathBuf};

use maybe_async::maybe_async;
use xdvdfs::write::{self, img::ProgressInfo};

fn get_default_image_path(source_path: &Path) -> Option<PathBuf> {
    let source_file_name = source_path.file_name()?;
    let output = PathBuf::from(source_file_name).with_extension("iso");

    if output.exists() && output == source_path {
        return Some(PathBuf::from(source_file_name).with_extension("xiso.iso"));
    }

    Some(output)
}

#[maybe_async]
pub async fn cmd_pack(
    source_path: &String,
    image_path: &Option<String>,
) -> Result<(), anyhow::Error> {
    let source_path = PathBuf::from(source_path);

    let image_path = image_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| get_default_image_path(&source_path).unwrap());

    let image = std::fs::File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(image_path)?;
    let mut image = std::io::BufWriter::with_capacity(1024 * 1024, image);

    let progress_callback = |pi| match pi {
        ProgressInfo::DirAdded(path, sector) => {
            println!("Added dir: {:?} at sector {}", path, sector);
        }
        ProgressInfo::FileAdded(path, sector) => {
            println!("Added file: {:?} at sector {}", path, sector);
        }
        _ => {}
    };

    let meta = std::fs::metadata(&source_path)?;
    if meta.is_dir() {
        let mut fs = write::fs::StdFilesystem;
        write::img::create_xdvdfs_image(&source_path, &mut fs, &mut image, progress_callback)
            .await?;
    } else if meta.is_file() {
        let source = crate::img::open_image_raw(&source_path).await?;
        let mut fs = write::fs::XDVDFSFilesystem::new(source)
            .await
            .ok_or(anyhow::anyhow!("Failed to create XDVDFS filesystem"))?;
        write::img::create_xdvdfs_image(
            &PathBuf::from("/"),
            &mut fs,
            &mut image,
            progress_callback,
        )
        .await?;
    } else {
        return Err(anyhow::anyhow!("Symlink image sources are not supported"));
    }

    Ok(())
}
