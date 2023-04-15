use std::path::PathBuf;

use xdvdfs::write;

pub fn cmd_pack(source_path: &String, image_path: &Option<String>) -> Result<(), String> {
    let source_path = PathBuf::from(source_path);
    let source_file_name = source_path.file_name().ok_or("Invalid source")?;
    let image_path = image_path
        .as_ref()
        .map(|ip| PathBuf::from(ip))
        .unwrap_or_else(|| PathBuf::from(source_file_name).with_extension("iso"));

    write::create_xdvdfs_image(&source_path, &image_path).map_err(|e| e.to_string())
}
