use log::{info, log_enabled, Level};
use std::{
    collections::HashMap,
    convert::Infallible,
    fs::{File, Metadata},
    io::BufReader,
    os::unix::fs::MetadataExt,
    path::Path,
    time::{Duration, SystemTime},
};

use tokio::runtime::Runtime;
use xdvdfs::{
    blockdev::OffsetWrapper,
    layout::{DirectoryEntryNode, DirectoryEntryTable, VolumeDescriptor},
};

struct FuseFilesystemCache {
    inode_lookup: HashMap<u64, DirectoryEntryNode>,
    inode_rev_lookup: HashMap<DirectoryEntryNode, u64>,
    next_inode: u64,
}

pub struct FuseFilesystem {
    device: OffsetWrapper<BufReader<File>>,
    src_atime: SystemTime,
    src_mtime: SystemTime,
    src_ctime: SystemTime,
    src_crtime: SystemTime,
    volume: VolumeDescriptor,
    rt: Runtime,
    cache: FuseFilesystemCache,
}

// An INode can resolve to either a non-root entry,
// the root entry (which has no on-disk Node), or
// no entry (if it does not exist).
// The majority of lookups will result in a value, so
// the large_enum_variant suggestion is not productive.
#[allow(clippy::large_enum_variant)]
pub enum INodeLookupResult {
    Value(DirectoryEntryNode),
    RootEntry,
    NoEntry,
}

impl INodeLookupResult {
    fn some<R, MapVal: FnOnce(DirectoryEntryNode) -> R, MapRoot: FnOnce() -> R>(
        self,
        map_val: MapVal,
        map_root: MapRoot,
    ) -> Option<R> {
        match self {
            Self::Value(val) => Some(map_val(val)),
            Self::RootEntry => Some(map_root()),
            Self::NoEntry => None,
        }
    }
}

impl FuseFilesystemCache {
    fn new() -> Self {
        Self {
            inode_lookup: HashMap::new(),
            inode_rev_lookup: HashMap::new(),
            next_inode: 2,
        }
    }

    fn get_or_assign_inode(&mut self, dirent: &DirectoryEntryNode) -> u64 {
        let inode = self.inode_rev_lookup.get(dirent);
        if let Some(inode) = inode {
            if log_enabled!(Level::Info) {
                let name = dirent.name_str::<Infallible>();
                if let Ok(name) = name {
                    info!("[inode] Lookup found {inode} for {name}");
                }
            }

            return *inode;
        }

        self.inode_rev_lookup.insert(*dirent, self.next_inode);
        self.inode_lookup.insert(self.next_inode, *dirent);
        let inode = self.next_inode;
        self.next_inode += 1;

        if log_enabled!(Level::Info) {
            let name = dirent.name_str::<Infallible>();
            if let Ok(name) = name {
                info!("[inode] Assigned {inode} for {name}");
            }
        }

        inode
    }
}

impl FuseFilesystem {
    pub fn new(img_path: &Path, metadata: Metadata, rt: Runtime) -> anyhow::Result<FuseFilesystem> {
        let img = File::open(img_path)?;
        let img = BufReader::new(img);
        let mut device = rt.block_on(OffsetWrapper::new(img))?;

        // FIXME: Default ctime/crtime to image pack time, if available
        let src_atime = metadata.accessed().unwrap_or(SystemTime::UNIX_EPOCH);
        let src_mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let src_ctime = SystemTime::UNIX_EPOCH
            + Duration::new(metadata.ctime() as u64, metadata.ctime_nsec() as u32);
        let src_crtime = metadata.created().unwrap_or(SystemTime::UNIX_EPOCH);

        let volume = rt.block_on(xdvdfs::read::read_volume(&mut device))?;

        Ok(Self {
            device,
            rt,
            src_atime,
            src_mtime,
            src_ctime,
            src_crtime,
            volume,
            cache: FuseFilesystemCache::new(),
        })
    }

    pub fn lookup_dirent_by_inode(&self, inode: u64) -> INodeLookupResult {
        if inode == 1 {
            return INodeLookupResult::RootEntry;
        }

        let inode = self.cache.inode_lookup.get(&inode);
        match inode {
            Some(val) => INodeLookupResult::Value(*val),
            None => INodeLookupResult::NoEntry,
        }
    }

    pub fn lookup_dirtab_by_inode(
        &mut self,
        inode: u64,
    ) -> anyhow::Result<Option<DirectoryEntryTable>> {
        let dirtab = if inode == 1 {
            Some(self.volume.root_table)
        } else {
            self.cache
                .inode_lookup
                .get(&inode)
                .and_then(|dirent| dirent.node.dirent.dirent_table())
        };

        Ok(dirtab)
    }

    fn new_file_attr(
        &self,
        req: &fuser::Request<'_>,
        ino: u64,
        byte_size: u64,
        is_dir: bool,
    ) -> fuser::FileAttr {
        let kind = if is_dir {
            fuser::FileType::Directory
        } else {
            fuser::FileType::RegularFile
        };

        fuser::FileAttr {
            ino,
            size: byte_size,
            blocks: byte_size.div_ceil(xdvdfs::layout::SECTOR_SIZE as u64),
            atime: self.src_atime,
            mtime: self.src_mtime,
            ctime: self.src_ctime,
            crtime: self.src_crtime,
            kind,
            perm: 0o444, // r--r--r--
            nlink: 0,
            uid: req.uid(),
            gid: req.gid(),
            rdev: 0,
            blksize: xdvdfs::layout::SECTOR_SIZE,
            flags: 0,
        }
    }
}

impl fuser::Filesystem for FuseFilesystem {
    fn lookup(
        &mut self,
        req: &fuser::Request<'_>,
        parent: u64,
        name: &std::ffi::OsStr,
        reply: fuser::ReplyEntry,
    ) {
        info!("[lookup] For {name:?} in inode {parent}");

        let Some(name) = name.to_str() else {
            reply.error(libc::EINVAL);
            return;
        };

        let Ok(dirtab) = self.lookup_dirtab_by_inode(parent) else {
            reply.error(libc::EIO);
            return;
        };

        let Some(dirtab) = dirtab else {
            reply.error(libc::ENOTDIR);
            return;
        };

        let dirent = self.rt.block_on(dirtab.walk_path(&mut self.device, name));
        let dirent = match dirent {
            Ok(dirent) => dirent,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let inode = self.cache.get_or_assign_inode(&dirent);
        let file_attr = self.new_file_attr(
            req,
            inode,
            dirent.node.dirent.data.size as u64,
            dirent.node.dirent.is_directory(),
        );

        reply.entry(&Duration::new(0, 0), &file_attr, 0);
    }

    fn getattr(
        &mut self,
        req: &fuser::Request<'_>,
        ino: u64,
        _fh: Option<u64>,
        reply: fuser::ReplyAttr,
    ) {
        info!("[getattr] for inode {ino}");

        let inode = self.lookup_dirent_by_inode(ino);
        let inode = inode.some(
            |ent| {
                self.new_file_attr(
                    req,
                    ino,
                    ent.node.dirent.data.size.into(),
                    ent.node.dirent.is_directory(),
                )
            },
            || self.new_file_attr(req, 1, self.volume.root_table.region.size as u64, true),
        );

        match inode {
            Some(attr) => reply.attr(&Duration::new(0, 0), &attr),
            None => reply.error(libc::ENOENT),
        }
    }

    fn open(&mut self, _req: &fuser::Request<'_>, _ino: u64, flags: i32, reply: fuser::ReplyOpen) {
        info!("[open] for inode {_ino}");

        let unsupported_flags = libc::O_WRONLY | libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC;
        if flags & unsupported_flags != 0 {
            reply.error(libc::ENOTSUP);
            return;
        }

        reply.opened(0, 0);
    }

    fn read(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyData,
    ) {
        info!("[read] for inode {ino}");

        let inode = self.lookup_dirent_by_inode(ino);
        let dirent = match inode {
            INodeLookupResult::Value(val) => val,
            INodeLookupResult::RootEntry => {
                reply.error(libc::EISDIR);
                return;
            }
            INodeLookupResult::NoEntry => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        if offset < 0 {
            reply.error(libc::EINVAL);
            return;
        }

        let data =
            dirent
                .node
                .dirent
                .read_data_offset(&mut self.device, size as u64, offset as u64);
        let data = self.rt.block_on(data);
        match data {
            Ok(data) => reply.data(data.as_ref()),
            Err(_) => reply.error(libc::EIO),
        }
    }

    fn opendir(
        &mut self,
        _req: &fuser::Request<'_>,
        _ino: u64,
        flags: i32,
        reply: fuser::ReplyOpen,
    ) {
        info!("[opendir] for inode {_ino}");

        let unsupported_flags = libc::O_WRONLY | libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC;
        if flags & unsupported_flags != 0 {
            reply.error(libc::ENOTSUP);
            return;
        }

        reply.opened(0, 0);
    }

    fn readdir(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: fuser::ReplyDirectory,
    ) {
        info!("[readdir] for inode {ino} with offset {offset}");

        let Ok(dirtab) = self.lookup_dirtab_by_inode(ino) else {
            reply.error(libc::EIO);
            return;
        };

        let Some(dirtab) = dirtab else {
            reply.error(libc::ENOTDIR);
            return;
        };

        let iter = self.rt.block_on(dirtab.scan_dirent_tree(&mut self.device));
        let Ok(mut iter) = iter else {
            reply.error(libc::EIO);
            return;
        };

        let mut idx = 0;

        // Increment idx + 1 because 0 is special in FUSE
        if offset == 0 && reply.add(ino, idx + 1, fuser::FileType::Directory, ".") {
            return;
        }
        idx += 1;

        if offset <= 1 && reply.add(ino, idx + 1, fuser::FileType::Directory, "..") {
            return;
        }
        idx += 1;

        while idx < offset {
            let next = self.rt.block_on(iter.next_entry());
            let Ok(next) = next else {
                reply.error(libc::EIO);
                return;
            };

            if next.is_none() {
                break;
            }

            idx += 1;
        }

        info!("[readdir] starting at index {idx}");

        loop {
            let next = self.rt.block_on(iter.next_entry());
            match next {
                Ok(Some(dirent)) => {
                    let inode = self.cache.get_or_assign_inode(&dirent);
                    let ftype = if dirent.node.dirent.is_directory() {
                        fuser::FileType::Directory
                    } else {
                        fuser::FileType::RegularFile
                    };
                    let name = dirent.name_str::<Infallible>();
                    let Ok(name) = name else {
                        continue;
                    };
                    let name: String = name.to_string();

                    info!("[readdir] push record (inode = {inode}, idx = {idx}, name = {name})");
                    if reply.add(inode, idx + 1, ftype, name) {
                        break;
                    }

                    idx += 1;
                }
                Ok(None) => break,
                Err(_) => {
                    reply.error(libc::EIO);
                    return;
                }
            }
        }

        reply.ok();
    }

    fn access(
        &mut self,
        _req: &fuser::Request<'_>,
        _ino: u64,
        _mask: i32,
        reply: fuser::ReplyEmpty,
    ) {
        info!("[access] for inode {_ino}");
        reply.ok();
    }
}
