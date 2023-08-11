use sha3::{Digest, Sha3_256};

use crate::{blockdev::BlockDeviceRead, layout::VolumeDescriptor, util};
use maybe_async::maybe_async;

#[maybe_async(?Send)]
pub async fn checksum<E>(
    dev: &mut impl BlockDeviceRead<E>,
    volume: &VolumeDescriptor,
) -> Result<[u8; 32], util::Error<E>> {
    let mut hasher = Sha3_256::new();

    let tree = volume.root_table.file_tree(dev).await?;
    for (dir, file) in tree {
        let is_dir = file.node.dirent.is_directory();
        let file_name = file.name_str()?;

        hasher.update(dir.as_bytes());
        hasher.update(b"/");
        hasher.update(file_name.as_bytes());

        if !is_dir {
            let data = file.node.dirent.read_data_all(dev).await?;
            hasher.update(data);
        }
    }

    let bytes = hasher.finalize();
    let output: [u8; 32] = bytes[..]
        .try_into()
        .expect("SHA256 output should be 32 bytes");
    Ok(output)
}
