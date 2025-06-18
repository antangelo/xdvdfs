use std::{collections::HashMap, convert::Infallible};

use log::{info, log_enabled, Level};
use xdvdfs::layout::DirectoryEntryNode;

pub struct INodeCache {
    inode_lookup: HashMap<u64, DirectoryEntryNode>,
    inode_rev_lookup: HashMap<DirectoryEntryNode, u64>,
    next_inode: u64,
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
    pub fn some<R, MapVal: FnOnce(DirectoryEntryNode) -> R, MapRoot: FnOnce() -> R>(
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

impl INodeCache {
    pub fn new() -> Self {
        Self {
            inode_lookup: HashMap::new(),
            inode_rev_lookup: HashMap::new(),
            next_inode: 2,
        }
    }

    pub fn lookup_inode(&self, inode: u64) -> Option<&DirectoryEntryNode> {
        self.inode_lookup.get(&inode)
    }

    pub fn get_or_assign_inode(&mut self, dirent: &DirectoryEntryNode) -> u64 {
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
