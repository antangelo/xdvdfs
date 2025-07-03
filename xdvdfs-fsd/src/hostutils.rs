use crate::fsproto::FileAttribute;
use std::fs::Metadata;
use std::time::SystemTime;

pub struct FileTime {
    pub atime: SystemTime,
    pub mtime: SystemTime,
    pub ctime: SystemTime,
    pub crtime: SystemTime,
}

#[cfg(unix)]
fn file_ctime(metadata: &Metadata) -> SystemTime {
    use std::os::unix::fs::MetadataExt;
    use std::time::Duration;
    SystemTime::UNIX_EPOCH + Duration::new(metadata.ctime() as u64, metadata.ctime_nsec() as u32)
}

#[cfg(not(unix))]
fn file_ctime(metadata: &Metadata) -> SystemTime {
    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH)
}

pub fn metadata_to_time(meta: &Metadata) -> FileTime {
    let atime = meta.accessed().unwrap_or(SystemTime::UNIX_EPOCH);
    let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let crtime = meta.created().unwrap_or(SystemTime::UNIX_EPOCH);

    FileTime {
        atime,
        mtime,
        ctime: file_ctime(meta),
        crtime,
    }
}

pub fn metadata_to_attr(inode: u64, meta: &Metadata) -> FileAttribute {
    let atime = meta.accessed().unwrap_or(SystemTime::UNIX_EPOCH);
    let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let crtime = meta.created().unwrap_or(SystemTime::UNIX_EPOCH);

    FileAttribute {
        inode,
        byte_size: meta.len(),
        block_size: 512,
        is_dir: meta.is_dir(),
        is_writeable: false, // FIXME: Support writeable filesystem passthrough
        //is_writeable: !meta.is_dir() && !meta.permissions().readonly(),
        atime,
        mtime,
        ctime: file_ctime(meta),
        crtime,
    }
}
