pub mod fsproto;

pub mod daemonize;
pub mod hostutils;
pub mod inode;
pub mod overlay_fs;

#[cfg(not(feature = "sync"))]
mod img_fs;

#[cfg(not(feature = "sync"))]
mod mount;

fn main() {
    env_logger::init();

    #[cfg(not(feature = "sync"))]
    let res = mount::mount_main();

    #[cfg(feature = "sync")]
    let res: anyhow::Result<()> = Err(anyhow::anyhow!(
        "xdvdfs-fsd is not supported with 'sync' feature enabled"
    ));

    if let Err(err) = res {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
