use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Window;

use maybe_async::maybe_async;

use xdvdfs::{
    blockdev,
    write::{self, img::ProgressInfo},
};

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum CisoProgressInfo {
    SectorCount(usize),
    SectorsDone(usize),
    Finished,
}

struct SplitStdFs;

type BufFile = std::io::BufWriter<std::fs::File>;
type BufFileSectorLinearFs<'a> = write::fs::SectorLinearBlockFilesystem<
    'a,
    write::fs::XDVDFSFilesystem<
        blockdev::OffsetWrapper<std::io::BufReader<std::fs::File>>,
        Box<[u8]>,
        write::fs::DefaultCopier<
            blockdev::OffsetWrapper<std::io::BufReader<std::fs::File>>,
            Box<[u8]>,
        >,
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

#[tauri::command]
pub async fn compress_image(
    window: Window,
    source_path: String,
    dest_path: String,
) -> Option<String> {
    let source_path = PathBuf::from(source_path);
    let dest_path = PathBuf::from(dest_path);

    let mut output = ciso::split::SplitOutput::new(SplitStdFs, dest_path);

    let progress_callback = |pi: ProgressInfo| {
        window
            .emit("progress_callback", pi)
            .expect("should be able to send event");
    };

    let mut sectors_done = 0;
    let progress_callback_compression = |pi| {
        window
            .emit(
                "compress_callback",
                match pi {
                    ciso::write::ProgressInfo::SectorCount(sc) => CisoProgressInfo::SectorCount(sc),
                    ciso::write::ProgressInfo::SectorFinished => {
                        sectors_done += 1;
                        if sectors_done % 739 == 0 {
                            let sd = sectors_done;
                            sectors_done = 0;
                            CisoProgressInfo::SectorsDone(sd)
                        } else {
                            return;
                        }
                    }
                    ciso::write::ProgressInfo::Finished => CisoProgressInfo::Finished,
                    _ => return,
                },
            )
            .expect("should be able to send event")
    };

    let meta = std::fs::metadata(&source_path).ok()?;
    if meta.is_dir() {
        let mut fs = write::fs::StdFilesystem::create(&source_path);
        let mut slbd = write::fs::SectorLinearBlockDevice::default();
        let mut slbfs: write::fs::SectorLinearBlockFilesystem<write::fs::StdFilesystem> =
            write::fs::SectorLinearBlockFilesystem::new(&mut fs);

        write::img::create_xdvdfs_image(&mut slbfs, &mut slbd, progress_callback)
            .await
            .ok()?;

        let mut input = write::fs::CisoSectorInput::new(slbd, slbfs);
        ciso::write::write_ciso_image(&mut input, &mut output, progress_callback_compression)
            .await
            .ok()?;
    } else if meta.is_file() {
        let source = std::fs::File::options().read(true).open(source_path).ok()?;
        let source = std::io::BufReader::new(source);
        let source = xdvdfs::blockdev::OffsetWrapper::new(source).await.ok()?;
        let mut fs = write::fs::XDVDFSFilesystem::new(source)
            .await
            .ok_or("Failed to create XDVDFS filesystem".to_string())
            .ok()?;
        let mut slbd = write::fs::SectorLinearBlockDevice::default();
        let mut slbfs: BufFileSectorLinearFs = write::fs::SectorLinearBlockFilesystem::new(&mut fs);

        write::img::create_xdvdfs_image(&mut slbfs, &mut slbd, progress_callback)
            .await
            .ok()?;

        let mut input = write::fs::CisoSectorInput::new(slbd, slbfs);
        ciso::write::write_ciso_image(&mut input, &mut output, progress_callback_compression)
            .await
            .ok()?;
    } else {
        return Some("Symlink image sources are not supported".to_string());
    }

    None
}
