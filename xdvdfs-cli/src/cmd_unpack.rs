use crate::img::open_image_raw;
use clap::Args;
use maybe_async::maybe_async;
use std::{
    fs::File,
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    str::FromStr,
};
use xdvdfs::{blockdev::OffsetWrapper, layout::DirectoryEntryTable};

#[maybe_async]
async fn copyout_directory(
    img: &mut OffsetWrapper<BufReader<File>, std::io::Error>,
    dest_dir: &Path,
    dirtab: &DirectoryEntryTable,
) -> Result<(), anyhow::Error> {
    let tree = dirtab.file_tree(img).await?;

    for (dir, dirent) in &tree {
        let dir = dir.trim_start_matches('/');
        let dirname = dest_dir.join(dir);
        let file_name = dirent.name_str::<std::io::Error>()?;
        let file_path = dirname.join(&*file_name);
        let is_dir = dirent.node.dirent.is_directory();

        println!(
            "Extracting {} {}",
            if is_dir { "directory" } else { "file" },
            file_path.display()
        );

        std::fs::create_dir_all(dirname)?;
        if dirent.node.dirent.is_directory() {
            std::fs::create_dir(file_path)?;
            continue;
        }

        if dirent.node.dirent.filename_length == 0 {
            eprintln!("WARNING: {:?} has an empty file name, skipping", file_path);
            continue;
        }

        let mut file = File::options()
            .write(true)
            .truncate(true)
            .create(true)
            .open(file_path)?;

        if dirent.node.dirent.is_empty() {
            continue;
        }

        dirent.node.dirent.seek_to(img)?;
        let data = img.get_ref().get_ref().try_clone();
        match data {
            Ok(data) => {
                let data = data.take(dirent.node.dirent.data.size as u64);
                let mut data = std::io::BufReader::new(data);
                std::io::copy(&mut data, &mut file)?;
            }
            Err(err) => {
                eprintln!("Error in fast path, falling back to slow path: {:?}", err);
                let data = dirent.node.dirent.read_data_all(img).await?;
                file.write_all(&data)?;
            }
        }
    }

    Ok(())
}

#[derive(Args)]
#[command(about = "Unpack an entire image to a directory")]
pub struct UnpackArgs {
    #[arg(help = "Path to XISO image")]
    image_path: String,

    #[arg(help = "Output directory")]
    path: Option<String>,
}

#[maybe_async]
pub async fn cmd_unpack(args: &UnpackArgs) -> Result<(), anyhow::Error> {
    let target_dir = match &args.path {
        Some(path) => PathBuf::from_str(path).unwrap(),
        None => {
            let os_path = PathBuf::from_str(&args.image_path).unwrap();
            PathBuf::from(os_path.file_name().unwrap()).with_extension("")
        }
    };

    let mut img = open_image_raw(Path::new(&args.image_path)).await?;
    let volume = xdvdfs::read::read_volume(&mut img).await?;

    copyout_directory(&mut img, &target_dir, &volume.root_table).await
}

#[derive(Args)]
#[command(about = "Copy a file or directory out of the provided image file")]
pub struct CopyOutArgs {
    #[arg(help = "Path to XISO image")]
    image: String,

    #[arg(help = "Path to source file/directory inside image")]
    src_path: String,

    #[arg(help = "Path to destination on the host")]
    dest_path: String,
}

#[maybe_async]
pub async fn cmd_copyout(args: &CopyOutArgs) -> Result<(), anyhow::Error> {
    let mut img = open_image_raw(Path::new(&args.image)).await?;
    let dest_path = Path::new(&args.dest_path);

    let volume = xdvdfs::read::read_volume(&mut img).await?;
    if args.src_path == "/" {
        return copyout_directory(&mut img, dest_path, &volume.root_table).await;
    }

    let dirent = volume
        .root_table
        .walk_path(&mut img, &args.src_path)
        .await?;
    if let Some(table) = dirent.node.dirent.dirent_table() {
        return copyout_directory(&mut img, dest_path, &table).await;
    }

    let mut file = File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(&args.dest_path)?;

    if dirent.node.dirent.is_empty() {
        return Ok(());
    }

    dirent.node.dirent.seek_to(&mut img)?;
    let data = img.get_ref().get_ref().try_clone();
    match data {
        Ok(data) => {
            let data = data.take(dirent.node.dirent.data.size as u64);
            let mut data = std::io::BufReader::new(data);
            std::io::copy(&mut data, &mut file)?;
        }
        Err(err) => {
            eprintln!("Error in fast path, falling back to slow path: {:?}", err);
            let data = dirent.node.dirent.read_data_all(&mut img).await?;
            file.write_all(&data)?;
        }
    }

    Ok(())
}
