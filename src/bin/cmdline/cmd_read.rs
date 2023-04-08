use std::fs::File;

use md5::{Md5, Digest};

pub fn cmd_ls(img_path: &str, dir_path: &str) -> Result<(), String> {
    let mut img = File::options()
        .read(true)
        .open(img_path)
        .map_err(|e| e.to_string())?;
    let volume = xdvdfs::read::read_volume(&mut img).ok_or("Failed to read volume")?;

    let dirent_table = if dir_path == "/" {
        volume.root_table
    } else {
        volume
            .root_table
            .walk_path(&mut img, dir_path)
            .ok_or("Failed to walk path")?
            .node
            .dirent
            .dirent_table()
            .ok_or("Not a directory")?
    };

    let listing = dirent_table.walk_dirent_tree(&mut img);

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
    let volume = xdvdfs::read::read_volume(&mut img).ok_or("Failed to read volume")?;

    let tree = volume.root_table.file_tree(&mut img);
    let mut total_size = 0;
    for (dir, file) in &tree {
        total_size += file.node.dirent.data.size();
        println!("{}/{} ({} bytes)", dir, file.get_name(), file.node.dirent.data.size());
    }

    println!("{} files, {} bytes", tree.len(), total_size);

    Ok(())
}

fn md5_file_dirent(img: &mut impl xdvdfs::blockdev::BlockDeviceRead,
                   file: xdvdfs::layout::DirectoryEntryNode) -> String {
    let file_pos = file.node.dirent.data;
    let file_buf: Vec<u8> = vec![0; file_pos.size() as usize];
    let mut file_buf = file_buf.into_boxed_slice();

    img.read(file_pos.offset(0).unwrap(), &mut file_buf);

    let mut hasher = Md5::new();
    hasher.update(file_buf);
    let result = hasher.finalize();

    format!("{:x}", result)
}

fn md5_file_tree(img: &mut impl xdvdfs::blockdev::BlockDeviceRead, tree: &Vec<(String, xdvdfs::layout::DirectoryEntryNode)>, base: &str) {
    for (dir, file) in tree {
        let dir = if base == "" {
            String::from(dir)
        } else if dir == "" {
            String::from(base)
        } else {
            format!("{}/{}", base, dir)
        };
        let checksum = md5_file_dirent(img, *file);
        println!("{}  {}/{}", checksum, dir, file.get_name());
    }
}

fn md5_from_file_path(volume: &xdvdfs::layout::VolumeDescriptor, img: &mut (impl xdvdfs::blockdev::BlockDeviceRead + std::io::Read), file: &str) -> Result<(), String> {
    let dirent = volume.root_table.walk_path(img, file).ok_or("File does not exist")?;
    if let Some(table) = dirent.node.dirent.dirent_table() {
        let tree = table.file_tree(img);
        md5_file_tree(img, &tree, file);
    } else {
        let checksum = md5_file_dirent(img, dirent);
        println!("{}  {}", checksum, file);
    }

    Ok(())
}

fn md5_from_root_tree(volume: &xdvdfs::layout::VolumeDescriptor, img: &mut impl xdvdfs::blockdev::BlockDeviceRead) -> Result<(), String> {
    let tree = volume.root_table.file_tree(img);
    md5_file_tree(img, &tree, "");

    Ok(())
}

pub fn cmd_md5(img_path: &str, path: Option<&str>) -> Result<(), String> {
    let mut img = File::options()
        .read(true)
        .open(img_path)
        .map_err(|e| e.to_string())?;
    let volume = xdvdfs::read::read_volume(&mut img).ok_or("Failed to read volume")?;

    if let Some(path) = path {
        md5_from_file_path(&volume, &mut img, path)
    } else {
        md5_from_root_tree(&volume, &mut img)
    }
}
