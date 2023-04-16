use std::path::PathBuf;

use xdvdfs::write;

pub fn cmd_pack(source_path: &String, image_path: &Option<String>) -> Result<(), String> {
    let source_path = PathBuf::from(source_path);
    let source_file_name = source_path.file_name().ok_or("Invalid source")?;
    let image_path = image_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(source_file_name).with_extension("iso"));

    let mut image = std::fs::File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(image_path)
        .map_err(|e| e.to_string())?;

    let fs = write::fs::StdFilesystem;

    write::img::create_xdvdfs_image(&source_path, &fs, &mut image).map_err(|e| e.to_string())
}
