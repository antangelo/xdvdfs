use std::fs::File;

use xdvdfs::layout::{DirectoryEntryNode, VolumeDescriptor};

fn print_volume(volume: &VolumeDescriptor) {
    let time = volume.filetime;
    let sector = volume.root_table.region.sector;
    let size = volume.root_table.region.size;

    println!("{0: <20} {1}", "Valid:", volume.is_valid());
    println!("{0: <20} {1}", "Creation time:", time);
    println!(
        "{0: <20} Sector {1} ({2} bytes)",
        "Root Entry:", sector, size
    );
}

fn print_dirent(dirent: &DirectoryEntryNode) {
    println!("{0: <20} {1}", "Name:", dirent.get_name());
    println!(
        "{0: <20} {1}",
        "Left Child Offset:",
        if dirent.node.left_entry_offset != 0 {
            format!("{} bytes", 4 * dirent.node.left_entry_offset)
        } else {
            String::from("None")
        }
    );
    println!(
        "{0: <20} {1}",
        "Right Child Offset:",
        if dirent.node.right_entry_offset != 0 {
            format!("{} bytes", 4 * dirent.node.right_entry_offset)
        } else {
            String::from("None")
        }
    );

    let sector = dirent.node.dirent.data.sector;
    let size = dirent.node.dirent.data.size;
    println!(
        "{0: <20} Sector {1} ({2} bytes)",
        "Data Location:", sector, size
    );

    println!("{0: <20} {1}", "Attributes:", dirent.node.dirent.attributes);
}

pub async fn cmd_info(img_path: &String, entry: Option<&String>) -> Result<(), String> {
    let mut img = File::options()
        .read(true)
        .open(img_path)
        .map_err(|e| e.to_string())?;
    let volume = xdvdfs::read::read_volume(&mut img)
        .await
        .map_err(|e| e.to_string())?;

    match entry {
        Some(path) => {
            let dirent = volume
                .root_table
                .walk_path(&mut img, path)
                .await
                .map_err(|e| e.to_string())?;
            print_dirent(&dirent);

            if let Some(subdir) = dirent.node.dirent.dirent_table() {
                println!();
                let children = subdir
                    .walk_dirent_tree(&mut img)
                    .await
                    .map_err(|e| e.to_string())?;
                for node in children {
                    println!("{}", node.get_name());
                    print_dirent(&node);
                    println!();
                }
            }
        }
        None => print_volume(&volume),
    }

    Ok(())
}
