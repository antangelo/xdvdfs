use std::fs::File;

pub fn cmd_patch(img_path: &String) -> Result<(), String> {
    let mut img = File::options()
        .read(true)
        .write(true)
        .open(img_path)
        .map_err(|e| e.to_string())?;
    let volume = xdvdfs::read::read_volume(&mut img).map_err(|e| e.to_string())?;

    let tree = volume
        .root_table
        .file_tree(&mut img)
        .map_err(|e| e.to_string())?;

    for (dir, file) in &tree {
        let name = file.get_name();
        let is_xbe = name.split('.').last().filter(|ext| *ext == "xbe").is_some();
        if !is_xbe {
            continue;
        }

        println!("Patching {}/{}", dir, name);
        xdvdfs::write::xbepatch::apply_media_patch(&mut img, file.node.dirent.data)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}
