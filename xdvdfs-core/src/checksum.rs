use alloc::{collections::BTreeMap, string::String};
use sha3::{Digest, Sha3_256};

use crate::{
    blockdev::BlockDeviceRead,
    layout::{DirectoryEntryNode, VolumeDescriptor},
    util,
};
use maybe_async::maybe_async;

#[maybe_async]
pub async fn checksum<E>(
    dev: &mut impl BlockDeviceRead<E>,
    volume: &VolumeDescriptor,
) -> Result<[u8; 32], util::Error<E>> {
    let mut hasher = Sha3_256::new();

    let tree = volume.root_table.file_tree(dev).await?;
    let mut iter: BTreeMap<String, DirectoryEntryNode> = BTreeMap::new();

    for (dir, file) in tree {
        let path = alloc::format!("{}/{}", dir, file.name_str()?);
        iter.insert(path, file);
    }

    for (path, file) in iter {
        let is_dir = file.node.dirent.is_directory();

        hasher.update(path.as_bytes());

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
