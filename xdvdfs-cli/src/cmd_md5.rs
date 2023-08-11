use maybe_async::maybe_async;
use md5::{Digest, Md5};
use std::fs::File;
use xdvdfs::util;

#[maybe_async(?Send)]
async fn md5_file_dirent<E>(
    img: &mut impl xdvdfs::blockdev::BlockDeviceRead<E>,
    file: xdvdfs::layout::DirectoryEntryNode,
) -> Result<String, util::Error<E>> {
    let file_buf = file.node.dirent.read_data_all(img).await?;

    let mut hasher = Md5::new();
    hasher.update(file_buf);
    let result = hasher.finalize();

    Ok(format!("{:x}", result))
}

#[maybe_async(?Send)]
async fn md5_file_tree<E>(
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
        let checksum = md5_file_dirent(img, *file).await?;
        let name = file.name_str()?;
        println!("{}  {}/{}", checksum, dir, name);
    }

    Ok(())
}

#[maybe_async(?Send)]
async fn md5_from_file_path<E>(
    volume: &xdvdfs::layout::VolumeDescriptor,
    img: &mut impl xdvdfs::blockdev::BlockDeviceRead<E>,
    file: &str,
) -> Result<(), util::Error<E>> {
    let dirent = volume.root_table.walk_path(img, file).await?;
    if let Some(table) = dirent.node.dirent.dirent_table() {
        let tree = table.file_tree(img).await?;
        md5_file_tree(img, &tree, file).await?;
    } else {
        let checksum = md5_file_dirent(img, dirent).await?;
        println!("{}  {}", checksum, file);
    }

    Ok(())
}

#[maybe_async(?Send)]
async fn md5_from_root_tree<E>(
    volume: &xdvdfs::layout::VolumeDescriptor,
    img: &mut impl xdvdfs::blockdev::BlockDeviceRead<E>,
) -> Result<(), util::Error<E>> {
    let tree = volume.root_table.file_tree(img).await?;
    md5_file_tree(img, &tree, "").await
}

#[maybe_async(?Send)]
pub async fn cmd_md5(img_path: &str, path: Option<&str>) -> Result<(), anyhow::Error> {
    let mut img = File::options().read(true).open(img_path)?;
    let volume = xdvdfs::read::read_volume(&mut img).await?;

    let result = if let Some(path) = path {
        md5_from_file_path(&volume, &mut img, path).await
    } else {
        md5_from_root_tree(&volume, &mut img).await
    };

    Ok(result?)
}
