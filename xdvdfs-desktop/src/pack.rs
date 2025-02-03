use std::path::PathBuf;
use tauri::Window;

#[tauri::command]
pub async fn pack_image(window: Window, source_path: String, dest_path: String) -> Option<String> {
    let source_path = PathBuf::from(source_path);
    let dest_path = PathBuf::from(dest_path);

    let image = std::fs::File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(dest_path)
        .map_err(|e| e.to_string())
        .ok()?;
    let mut image = std::io::BufWriter::with_capacity(1024 * 1024, image);

    let progress_callback = |pi| {
        window
            .emit("progress_callback", pi)
            .expect("should be able to send event");
    };

    let meta = std::fs::metadata(&source_path)
        .map_err(|e| e.to_string())
        .ok()?;
    if meta.is_dir() {
        let mut fs = xdvdfs::write::fs::StdFilesystem::create(&source_path);
        xdvdfs::write::img::create_xdvdfs_image(&mut fs, &mut image, progress_callback)
            .await
            .ok()?;
    } else if meta.is_file() {
        let source = std::fs::File::options().read(true).open(source_path).ok()?;
        let source = std::io::BufReader::new(source);
        let source = xdvdfs::blockdev::OffsetWrapper::new(source).await.ok()?;
        let mut fs =
            xdvdfs::write::fs::XDVDFSFilesystem::<_, _, xdvdfs::write::fs::StdIOCopier<_, _>>::new(
                source,
            )
            .await
            .ok_or("Failed to create XDVDFS filesystem".to_string())
            .ok()?;
        xdvdfs::write::img::create_xdvdfs_image(&mut fs, &mut image, progress_callback)
            .await
            .ok()?;
    } else {
        return Some("Symlink image sources are not supported".to_string());
    }

    None
}
