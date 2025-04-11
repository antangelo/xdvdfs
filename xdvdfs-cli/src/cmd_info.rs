use std::path::Path;

use clap::Args;
use maybe_async::maybe_async;
use xdvdfs::blockdev::BlockDeviceRead;
use xdvdfs::layout::{DirectoryEntryNode, DirectoryEntryTable, VolumeDescriptor};

#[derive(Args)]
#[command(
    about = "Print information about image metadata",
    long_about = "\
    Print information about image metadata. \
    If a file is specified, prints its directory entry. \
    If no file is specified, prints volume metadata."
)]
pub struct InfoArgs {
    #[arg(help = "Path to XISO image")]
    image_path: String,

    #[arg(help = "Path to file/directory within image")]
    file_entry: Option<String>,
}

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

fn print_dirent(dirent: &DirectoryEntryNode) -> Result<(), anyhow::Error> {
    let name = dirent.name_str::<std::io::Error>()?;
    println!("{0: <20} {1}", "Name:", name);
    println!("{0: <20} {1}", "Offset:", dirent.offset);
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
    Ok(())
}

#[maybe_async]
async fn print_subdir(
    subdir: &DirectoryEntryTable,
    img: &mut impl BlockDeviceRead<ReadError = std::io::Error>,
) -> Result<(), anyhow::Error> {
    let children = subdir.walk_dirent_tree(img).await?;
    for node in children {
        let name = node.name_str::<std::io::Error>()?;
        println!("{}", name);
        print_dirent(&node)?;
        println!();
    }

    Ok(())
}

#[maybe_async]
pub async fn cmd_info(args: &InfoArgs) -> Result<(), anyhow::Error> {
    let image_path = Path::new(&args.image_path);

    if let Some(path) = &args.file_entry {
        let mut image = crate::img::open_image(image_path).await?;
        let volume = xdvdfs::read::read_volume(&mut image).await?;

        if path == "/" {
            print_subdir(&volume.root_table, &mut image).await?;
            return Ok(());
        }

        let dirent = volume.root_table.walk_path(&mut image, path).await?;
        print_dirent(&dirent)?;

        if let Some(subdir) = dirent.node.dirent.dirent_table() {
            println!();
            print_subdir(&subdir, &mut image).await?;
        }

        return Ok(());
    }

    let volume = if image_path.extension().is_some_and(|e| e == "cso") {
        let mut image = crate::img::open_image(image_path).await?;
        println!("{:<20} CISO", "Image type:");
        xdvdfs::read::read_volume(&mut image).await?
    } else {
        let mut image = crate::img::open_image_raw(image_path).await?;
        let xgd_type = image.get_offset();

        println!(
            "{:<20} {:?} ({} byte offset)",
            "Image type:",
            xgd_type,
            u64::from(xgd_type)
        );

        xdvdfs::read::read_volume(&mut image).await?
    };

    print_volume(&volume);
    Ok(())
}
