use md5::{Digest, Md5};
use std::fs::File;
use xdvdfs::util;

fn md5_file_dirent<E>(
    img: &mut impl xdvdfs::blockdev::BlockDeviceRead<E>,
    file: xdvdfs::layout::DirectoryEntryNode,
) -> Result<String, util::Error<E>> {
    let file_buf = file.node.dirent.read_data_all(img)?;

    let mut hasher = Md5::new();
    hasher.update(file_buf);
    let result = hasher.finalize();

    Ok(format!("{:x}", result))
}

fn md5_file_tree<E>(
    img: &mut impl xdvdfs::blockdev::BlockDeviceRead<E>,
    tree: &Vec<(String, xdvdfs::layout::DirectoryEntryNode)>,
    base: &str,
) -> Result<(), util::Error<E>> {
    for (dir, file) in tree {
        let dir = if base.is_empty() {
            String::from(dir)
        } else if dir.is_empty() {
            String::from(base)
        } else {
            format!("{}/{}", base, dir)
        };
        let checksum = md5_file_dirent(img, *file)?;
        println!("{}  {}/{}", checksum, dir, file.get_name());
    }

    Ok(())
}

fn md5_from_file_path<E>(
    volume: &xdvdfs::layout::VolumeDescriptor,
    img: &mut impl xdvdfs::blockdev::BlockDeviceRead<E>,
    file: &str,
) -> Result<(), util::Error<E>> {
    let dirent = volume.root_table.walk_path(img, file)?;
    if let Some(table) = dirent.node.dirent.dirent_table() {
        let tree = table.file_tree(img)?;
        md5_file_tree(img, &tree, file)?;
    } else {
        let checksum = md5_file_dirent(img, dirent)?;
        println!("{}  {}", checksum, file);
    }

    Ok(())
}

fn md5_from_root_tree<E>(
    volume: &xdvdfs::layout::VolumeDescriptor,
    img: &mut impl xdvdfs::blockdev::BlockDeviceRead<E>,
) -> Result<(), util::Error<E>> {
    let tree = volume.root_table.file_tree(img)?;
    md5_file_tree(img, &tree, "")
}

pub fn cmd_md5(img_path: &str, path: Option<&str>) -> Result<(), String> {
    let mut img = File::options()
        .read(true)
        .open(img_path)
        .map_err(|e| e.to_string())?;
    let volume = xdvdfs::read::read_volume(&mut img).map_err(|e| e.to_string())?;

    let result = if let Some(path) = path {
        md5_from_file_path(&volume, &mut img, path)
    } else {
        md5_from_root_tree(&volume, &mut img)
    };

    result.map_err(|e| e.to_string())
}
