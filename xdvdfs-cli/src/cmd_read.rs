use crate::img::open_image;
use clap::Args;
use maybe_async::maybe_async;
use std::path::Path;

#[derive(Args)]
#[command(about = "List files in an image")]
pub struct LsArgs {
    #[arg(help = "Path to XISO image")]
    image_path: String,

    #[arg(default_value = "/", help = "Directory to list")]
    path: String,

    #[arg(short = 's', long = "scan", help = "Scan")]
    scan: bool,
}

#[maybe_async]
pub async fn cmd_ls(args: &LsArgs) -> Result<(), anyhow::Error> {
    let mut img = open_image(Path::new(&args.image_path)).await?;
    let volume = xdvdfs::read::read_volume(&mut img).await?;

    let dirent_table = if args.path == "/" {
        volume.root_table
    } else {
        volume
            .root_table
            .walk_path(&mut img, &args.path)
            .await?
            .node
            .dirent
            .dirent_table()
            .ok_or(anyhow::anyhow!("Not a directory"))?
    };

    if args.scan {
        let mut iter = dirent_table.scan_dirent_tree(&mut img).await?;

        while let Some(dirent) = iter.next().await? {
            let name = dirent.name_str::<std::io::Error>()?;
            println!("{}", name);
        }

        return Ok(());
    }

    let listing = dirent_table.walk_dirent_tree(&mut img).await?;

    for dirent in listing {
        let name = dirent.name_str::<std::io::Error>()?;
        println!("{}", name);
    }

    Ok(())
}

#[derive(Args)]
#[command(about = "List all files in an image, recursively")]
pub struct TreeArgs {
    #[arg(help = "Path to XISO image")]
    image_path: String,
}

#[maybe_async]
pub async fn cmd_tree(args: &TreeArgs) -> Result<(), anyhow::Error> {
    let mut img = open_image(Path::new(&args.image_path)).await?;
    let volume = xdvdfs::read::read_volume(&mut img).await?;

    let tree = volume.root_table.file_tree(&mut img).await?;

    let mut total_size: usize = 0;
    let mut file_count: usize = 0;

    for (dir, file) in &tree {
        let is_dir = file.node.dirent.is_directory();
        let name = file.name_str::<std::io::Error>()?;
        if is_dir {
            println!("{}/{}/ (0 bytes)", dir, name);
        } else {
            total_size += file.node.dirent.data.size() as usize;
            file_count += 1;

            println!("{}/{} ({} bytes)", dir, name, file.node.dirent.data.size());
        }
    }

    println!("{} files, {} bytes", file_count, total_size);

    Ok(())
}

#[derive(Args)]
#[command(about = "Compute deterministic checksum of image contents")]
pub struct ChecksumArgs {
    #[arg(help = "Path to XISO image")]
    images: Vec<String>,

    #[arg(short, long, help = "Only output checksums without warnings")]
    silent: bool,
}

#[maybe_async]
async fn checksum_single(img_path: &str) -> Result<(), anyhow::Error> {
    let mut img = open_image(Path::new(img_path)).await?;
    let volume = xdvdfs::read::read_volume(&mut img).await?;
    let checksum = xdvdfs::checksum::checksum(&mut img, &volume).await?;

    for byte in checksum {
        print!("{:02x}", byte);
    }

    println!("\t{}", img_path);
    Ok(())
}

#[maybe_async]
pub async fn cmd_checksum(args: &ChecksumArgs) -> Result<(), anyhow::Error> {
    if !args.silent {
        eprintln!("This SHA256 sum is a condensed checksum of the all the data inside the image");
        eprintln!("It does not encode information about the filesystem structure outside of the data being in the correct order.");
        eprintln!("Note that this is NOT a SHA256 sum of the full image, and cannot be compared to a SHA256 sum of the full image.");
        eprintln!(
            "This checksum is only useful when compared to other checksums created by this tool."
        );
        eprintln!();
    }

    for image in &args.images {
        checksum_single(image).await?;
    }

    Ok(())
}
