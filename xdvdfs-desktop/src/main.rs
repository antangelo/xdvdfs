// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod pack;
mod unpack;
mod compress;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            pack::pack_image,
            unpack::unpack_image,
            compress::compress_image
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
