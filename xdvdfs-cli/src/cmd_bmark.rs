use std::{
    error::Error,
    fmt::{Debug, Display},
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::bail;
use clap::Args;
use maybe_async::maybe_async;
use xdvdfs::{
    blockdev::{DefaultCopier, NullBlockDevice, NullCopier},
    write::{
        self,
        fs::{FilesystemCopier, FilesystemHierarchy},
        img::ProgressInfo,
    },
};

#[derive(Clone, Default, clap::ValueEnum)]
enum BlockDeviceType {
    #[default]
    Null,
    Memory,
}

impl std::fmt::Display for BlockDeviceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Memory => f.write_str("memory"),
        }
    }
}

#[derive(Args)]
#[command(about = "Benchmark", hide = true)]
pub struct BmarkArgs {
    #[arg(help = "Path to source directory or ISO image")]
    source_path: String,

    #[arg(
        help = "Number of iterations",
        default_value_t = 5,
        short = 'i',
        required = false
    )]
    iterations: usize,

    #[arg(
        help = "Block device type to pack into",
        default_value_t = BlockDeviceType::default(),
        short = 'b',
    )]
    blockdev: BlockDeviceType,
}

#[derive(Clone, Debug)]
struct BenchmarkMeasurement {
    file_count: usize,
    dir_count: usize,

    start_time: Instant,
    last_msg_time: Duration,
    backward_pass_end: Duration,
    forward_pass_end: Duration,
}

impl Default for BenchmarkMeasurement {
    fn default() -> Self {
        Self {
            file_count: 0,
            dir_count: 0,
            start_time: Instant::now(),
            last_msg_time: Duration::default(),
            backward_pass_end: Duration::default(),
            forward_pass_end: Duration::default(),
        }
    }
}

impl Display for BenchmarkMeasurement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[ {} files, {} directories, backward pass: {:?}, forward pass: {:?} ]",
            self.file_count,
            self.dir_count,
            self.backward_pass_end,
            self.forward_pass_end.saturating_sub(self.backward_pass_end),
        )
    }
}

impl BenchmarkMeasurement {
    fn get_progress_callback(&mut self) -> impl FnMut(ProgressInfo) + '_ {
        self.start_time = Instant::now();
        |pi| {
            let msg_time = self.start_time.elapsed();
            match pi {
                ProgressInfo::FileCount(count) => {
                    self.file_count += count;
                }
                ProgressInfo::DirCount(count) => {
                    self.dir_count += count;
                    self.backward_pass_end = msg_time;
                }
                ProgressInfo::FinishedCopyingImageData => self.forward_pass_end = msg_time,
                _ => {}
            }

            self.last_msg_time = msg_time;
        }
    }
}

#[maybe_async]
async fn run_sector_linear_pack_iteration<
    E1: Error + Debug + Display + Send + Sync + 'static,
    FS: FilesystemHierarchy<Error = E1> + FilesystemCopier<[u8]>,
>(
    fs: &mut FS,
    iterations: usize,
) -> anyhow::Result<Vec<BenchmarkMeasurement>> {
    let mut output = vec![BenchmarkMeasurement::default(); iterations];
    let mut slbfs = write::fs::SectorLinearBlockFilesystem::new(fs);

    for (idx, bmm) in output.iter_mut().enumerate() {
        let mut slbd = write::fs::SectorLinearBlockDevice::default();

        let progress_cb = bmm.get_progress_callback();
        let result = write::img::create_xdvdfs_image(&mut slbfs, &mut slbd, progress_cb).await;

        if let Err(e) = result {
            eprintln!("Error in iteration {idx}: {e:?}");
        }

        slbfs.clear_cache().await?;
    }

    Ok(output)
}

#[maybe_async]
async fn run_null_backend_pack_iteration<
    E1: Error + Debug + Display + Send + Sync + 'static,
    E2: Error + Debug + Display + Send + Sync + 'static,
    FS: FilesystemHierarchy<Error = E1> + FilesystemCopier<NullBlockDevice, Error = E2>,
>(
    fs: &mut FS,
    iterations: usize,
) -> anyhow::Result<Vec<BenchmarkMeasurement>> {
    let mut output = vec![BenchmarkMeasurement::default(); iterations];

    for (idx, bmm) in output.iter_mut().enumerate() {
        let mut nullbd = NullBlockDevice::default();
        let progress_cb = bmm.get_progress_callback();

        let result = write::img::create_xdvdfs_image(fs, &mut nullbd, progress_cb).await;
        if let Err(e) = result {
            eprintln!("Error in iteration {idx}: {e:?}");
        }

        fs.clear_cache().await?;
    }

    Ok(output)
}

#[maybe_async]
pub async fn cmd_bmark(args: &BmarkArgs) -> anyhow::Result<()> {
    let source_path = PathBuf::from(&args.source_path);

    let meta = std::fs::metadata(&source_path)?;
    let is_dir = meta.is_dir();

    let result = if is_dir {
        let mut fs = write::fs::StdFilesystem::create(&source_path);

        match args.blockdev {
            BlockDeviceType::Null => {
                run_null_backend_pack_iteration(&mut fs, args.iterations).await
            }
            BlockDeviceType::Memory => {
                run_sector_linear_pack_iteration(&mut fs, args.iterations).await
            }
        }
    } else if meta.is_file() {
        let source = crate::img::open_image_raw(&source_path).await?;

        match args.blockdev {
            BlockDeviceType::Null => {
                let mut fs =
                    write::fs::XDVDFSFilesystem::<_, _, NullCopier<_>>::new(source)
                        .await
                        .ok_or(anyhow::anyhow!("Failed to create XDVDFS filesystem"))?;
                run_null_backend_pack_iteration(&mut fs, args.iterations).await
            }
            BlockDeviceType::Memory => {
                let mut fs =
                    write::fs::XDVDFSFilesystem::<_, _, DefaultCopier<_, _>>::new(
                        source,
                    )
                    .await
                    .ok_or(anyhow::anyhow!("Failed to create XDVDFS filesystem"))?;
                run_sector_linear_pack_iteration(&mut fs, args.iterations).await
            }
        }
    } else {
        bail!("Symlink image sources are not supported");
    };

    for (iter, result) in result?.into_iter().enumerate() {
        println!("Iteration {iter}: {result}");
    }

    Ok(())
}
