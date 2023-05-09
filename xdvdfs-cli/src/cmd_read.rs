use std::{
    fs::File,
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    str::FromStr,
};
use xdvdfs::blockdev::OffsetWrapper;

pub async fn open_image(
    path: &Path,
) -> Result<OffsetWrapper<BufReader<File>, std::io::Error>, anyhow::Error> {
    let img = File::options().read(true).open(path)?;
    let img = std::io::BufReader::new(img);
    Ok(xdvdfs::blockdev::OffsetWrapper::new(img).await?)
}

pub async fn cmd_ls(img_path: &str, dir_path: &str) -> Result<(), anyhow::Error> {
    let mut img = open_image(Path::new(img_path)).await?;
    let volume = xdvdfs::read::read_volume(&mut img).await?;

    let dirent_table = if dir_path == "/" {
        volume.root_table
    } else {
        volume
            .root_table
            .walk_path(&mut img, dir_path)
            .await?
            .node
            .dirent
            .dirent_table()
            .ok_or(anyhow::anyhow!("Not a directory"))?
    };

    let listing = dirent_table.walk_dirent_tree(&mut img).await?;

    for dirent in listing {
        let name = dirent.name_str::<std::io::Error>()?;
        println!("{}", name);
    }

    Ok(())
}

pub async fn cmd_tree(img_path: &str) -> Result<(), anyhow::Error> {
    let mut img = open_image(Path::new(img_path)).await?;
    let volume = xdvdfs::read::read_volume(&mut img).await?;

    let tree = volume.root_table.file_tree(&mut img).await?;

    let mut total_size: usize = 0;
    let mut file_count: usize = 0;

    for (dir, file) in &tree {
        let is_dir = file.node.dirent.is_directory();
        let name = file.name_str::<std::io::Error>()?;
        if is_dir {
            println!("{}/{}/ (0 bytes)", dir, name);
        } else {
            total_size += file.node.dirent.data.size() as usize;
            file_count += 1;

            println!("{}/{} ({} bytes)", dir, name, file.node.dirent.data.size());
        }
    }

    println!("{} files, {} bytes", file_count, total_size);

    Ok(())
}

pub async fn cmd_unpack(img_path: &str, target_dir: &Option<String>) -> Result<(), anyhow::Error> {
    let target_dir = match target_dir {
        Some(path) => PathBuf::from_str(path).unwrap(),
        None => {
            let os_path = PathBuf::from_str(img_path).unwrap();
            PathBuf::from(os_path.file_name().unwrap()).with_extension("")
        }
    };

    let mut img = open_image(Path::new(img_path)).await?;
    let volume = xdvdfs::read::read_volume(&mut img).await?;
    let tree = volume.root_table.file_tree(&mut img).await?;

    for (dir, dirent) in &tree {
        let dir = dir.trim_start_matches('/');
        let dirname = target_dir.join(dir);
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

        dirent.node.dirent.seek_to(img.get_mut())?;
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
    }

    Ok(())
}
