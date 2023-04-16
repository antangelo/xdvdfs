use std::{fs::File, io::Write, path::Path};

pub fn cmd_ls(img_path: &str, dir_path: &str) -> Result<(), String> {
    let mut img = File::options()
        .read(true)
        .open(img_path)
        .map_err(|e| e.to_string())?;
    let volume = xdvdfs::read::read_volume(&mut img).map_err(|e| e.to_string())?;

    let dirent_table = if dir_path == "/" {
        volume.root_table
    } else {
        volume
            .root_table
            .walk_path(&mut img, dir_path)
            .map_err(|e| e.to_string())?
            .node
            .dirent
            .dirent_table()
            .ok_or("Not a directory")?
    };

    let listing = dirent_table
        .walk_dirent_tree(&mut img)
        .map_err(|e| e.to_string())?;

    for dirent in listing {
        println!("{}", dirent.get_name());
    }

    Ok(())
}

pub fn cmd_tree(img_path: &str) -> Result<(), String> {
    let mut img = File::options()
        .read(true)
        .open(img_path)
        .map_err(|e| e.to_string())?;
    let volume = xdvdfs::read::read_volume(&mut img).map_err(|e| e.to_string())?;

    let tree = volume
        .root_table
        .file_tree(&mut img)
        .map_err(|e| e.to_string())?;
    let mut total_size: usize = 0;
    for (dir, file) in &tree {
        total_size += file.node.dirent.data.size() as usize;
        println!(
            "{}/{} ({} bytes)",
            dir,
            file.get_name(),
            file.node.dirent.data.size()
        );
    }

    println!("{} files, {} bytes", tree.len(), total_size);

    Ok(())
}

pub fn cmd_unpack(img_path: &str, target_dir: &Path) -> Result<(), String> {
    let mut img = File::options()
        .read(true)
        .open(img_path)
        .map_err(|e| e.to_string())?;
    let volume = xdvdfs::read::read_volume(&mut img).map_err(|e| e.to_string())?;
    let tree = volume
        .root_table
        .file_tree(&mut img)
        .map_err(|e| e.to_string())?;

    for (dir, dirent) in &tree {
        let dir = dir.trim_start_matches('/');
        let dirname = target_dir.join(dir);
        let file_path = dirname.join(dirent.get_name());

        println!("Extracting file {}", file_path.display());

        std::fs::create_dir_all(dirname).map_err(|e| e.to_string())?;
        let mut file = File::options()
            .write(true)
            .truncate(true)
            .create(true)
            .open(file_path)
            .map_err(|e| e.to_string())?;

        let data = dirent
            .node
            .dirent
            .read_data_all(&mut img)
            .map_err(|e| e.to_string())?;
        file.write_all(&data).map_err(|e| e.to_string())?;
    }

    Ok(())
}
