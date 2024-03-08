use std::path::{Path, PathBuf};

use maybe_async::maybe_async;

use xdvdfs::{
    blockdev,
    write::{self, img::ProgressInfo},
};

use crate::img::open_image_raw;

struct SplitStdFs;

type BufFile = std::io::BufWriter<std::fs::File>;
type BufFileSectorLinearFs<'a> = write::fs::SectorLinearBlockFilesystem<
    'a,
    std::io::Error,
    std::fs::File,
    write::fs::XDVDFSFilesystem<
        std::io::Error,
        blockdev::OffsetWrapper<std::io::BufReader<std::fs::File>, std::io::Error>,
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

fn get_default_image_path(source_path: &Path) -> Option<PathBuf> {
    let source_file_name = source_path.file_name()?;
    let output = PathBuf::from(source_file_name).with_extension("cso");

    Some(output)
}

#[maybe_async]
pub async fn cmd_compress(
    source_path: &String,
    image_path: &Option<String>,
) -> Result<(), anyhow::Error> {
    let source_path = PathBuf::from(source_path);

    let image_path = image_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| get_default_image_path(&source_path).unwrap());

    // This is unlikely to happen, since compressed input is unsupported
    // and this will fail anyway, but we check to avoid truncating the input accidentally
    if image_path.exists() && image_path.canonicalize()? == source_path {
        return Err(anyhow::anyhow!("Source and destination paths are the same"));
    }

    let mut output = ciso::split::SplitOutput::new(SplitStdFs, image_path);

    let progress_callback = |pi| match pi {
        ProgressInfo::DirAdded(path, sector) => {
            println!("Added dir: {:?} at sector {}", path, sector);
        }
        ProgressInfo::FileAdded(path, sector) => {
            println!("Added file: {:?} at sector {}", path, sector);
        }
        _ => {}
    };

    let mut total_sectors = 0;
    let mut sectors_finished = 0;
    let progress_callback_compression = |pi| match pi {
        ciso::write::ProgressInfo::SectorCount(c) => total_sectors = c,
        ciso::write::ProgressInfo::SectorFinished => {
            sectors_finished += 1;
            print!(
                "\rCompressing sectors ({}/{})",
                sectors_finished, total_sectors
            );
        }
        ciso::write::ProgressInfo::Finished => println!(),
        _ => {}
    };

    let meta = std::fs::metadata(&source_path)?;
    if meta.is_dir() {
        let mut fs = write::fs::StdFilesystem;
        let mut slbd = write::fs::SectorLinearBlockDevice::default();
        let mut slbfs: write::fs::SectorLinearBlockFilesystem<
            std::io::Error,
            std::fs::File,
            write::fs::StdFilesystem,
        > = write::fs::SectorLinearBlockFilesystem::new(&mut fs);

        write::img::create_xdvdfs_image(&source_path, &mut slbfs, &mut slbd, progress_callback)
            .await?;

        let mut input = write::fs::CisoSectorInput::new(slbd, slbfs);
        ciso::write::write_ciso_image(&mut input, &mut output, progress_callback_compression)
            .await?;
    } else if meta.is_file() {
        let source = open_image_raw(&source_path).await?;
        let mut fs = write::fs::XDVDFSFilesystem::new(source)
            .await
            .ok_or(anyhow::anyhow!("Failed to create XDVDFS filesystem"))?;
        let mut slbd = write::fs::SectorLinearBlockDevice::default();
        let mut slbfs: BufFileSectorLinearFs = write::fs::SectorLinearBlockFilesystem::new(&mut fs);

        write::img::create_xdvdfs_image(
            &PathBuf::from("/"),
            &mut slbfs,
            &mut slbd,
            progress_callback,
        )
        .await?;

        let mut input = write::fs::CisoSectorInput::new(slbd, slbfs);
        ciso::write::write_ciso_image(&mut input, &mut output, progress_callback_compression)
            .await
            .unwrap();
    } else {
        return Err(anyhow::anyhow!("Symlink image sources are not supported"));
    }

    Ok(())
}
