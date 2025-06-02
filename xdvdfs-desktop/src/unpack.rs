use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use tauri::Window;
use xdvdfs::write::img::ProgressInfo;

#[tauri::command]
pub async fn unpack_image(
    window: Window,
    source_path: String,
    dest_path: String,
) -> Option<String> {
    let source_path = PathBuf::from(source_path);
    let dest_path = PathBuf::from(dest_path);

    let img = std::fs::File::options().read(true).open(source_path).ok()?;
    let img = std::io::BufReader::new(img);
    let mut img = xdvdfs::blockdev::OffsetWrapper::new(img).await.ok()?;

    let volume = xdvdfs::read::read_volume(&mut img).await.ok()?;
    let tree = volume.root_table.file_tree(&mut img).await.ok()?;

    window
        .emit("progress_callback", ProgressInfo::FileCount(tree.len()))
        .expect("should be able to send event");

    for (dir, dirent) in &tree {
        let dir = dir.trim_start_matches('/');
        let dirname = dest_path.join(dir);
        let file_name = dirent.name_str::<std::io::Error>().ok()?;
        let file_path = dirname.join(&*file_name);

        window
            .emit(
                "progress_callback",
                ProgressInfo::FileAdded(file_path.to_string_lossy().to_string(), 0),
            )
            .expect("should be able to send event");

        std::fs::create_dir_all(dirname).ok()?;
        if dirent.node.dirent.is_directory() {
            std::fs::create_dir(file_path).ok()?;
            continue;
        }

        if dirent.node.dirent.filename_length == 0 {
            continue;
        }

        let mut file = std::fs::File::options()
            .write(true)
            .truncate(true)
            .create(true)
            .open(file_path)
            .ok()?;

        if dirent.node.dirent.is_empty() {
            continue;
        }

        dirent.node.dirent.seek_to(&mut img).ok()?;
        let data = img.get_ref().get_ref().try_clone();
        match data {
            Ok(data) => {
                let data = data.take(dirent.node.dirent.data.size as u64);
                let mut data = std::io::BufReader::new(data);
                std::io::copy(&mut data, &mut file).ok()?;
            }
            Err(err) => {
                eprintln!("Error in fast path, falling back to slow path: {err:?}");
                let data = dirent.node.dirent.read_data_all(&mut img).await.ok()?;
                file.write_all(&data).ok()?;
            }
        }
    }

    None
}
