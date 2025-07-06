use std::{cmp::Eq, collections::HashMap, hash::Hash};

pub struct INodeCache<T: Eq + Hash + Clone> {
    inode_lookup: HashMap<u64, T>,
    inode_rev_lookup: HashMap<T, u64>,
    next_inode: u64,
}

// An INode can resolve to either a non-root entry,
// the root entry (which has no on-disk Node), or
// no entry (if it does not exist).
// The majority of lookups will result in a value, so
// the large_enum_variant suggestion is not productive.
#[allow(clippy::large_enum_variant)]
pub enum INodeLookupResult<T> {
    Value(T),
    RootEntry,
    NoEntry,
}

impl<T> INodeLookupResult<T> {
    pub fn some<R, MapVal: FnOnce(T) -> R, MapRoot: FnOnce() -> R>(
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

impl<T: Eq + Hash + Clone> Default for INodeCache<T> {
    fn default() -> Self {
        Self {
            inode_lookup: HashMap::new(),
            inode_rev_lookup: HashMap::new(),
            next_inode: 2,
        }
    }
}

impl<T: Eq + Hash + Clone> INodeCache<T> {
    pub fn lookup_inode(&self, inode: u64) -> Option<&T> {
        self.inode_lookup.get(&inode)
    }

    pub fn get_or_assign_inode(&mut self, dirent: &T) -> u64 {
        let inode = self.inode_rev_lookup.get(dirent);
        if let Some(inode) = inode {
            return *inode;
        }

        self.inode_rev_lookup
            .insert(dirent.clone(), self.next_inode);
        self.inode_lookup.insert(self.next_inode, dirent.clone());
        let inode = self.next_inode;
        self.next_inode += 1;

        inode
    }
}
