use core::fmt::Debug;
use core::slice::Iter;
use std::borrow::ToOwned;
use std::format;
use std::path::{Path, PathBuf};

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::blockdev::BlockDeviceWrite;

use maybe_async::maybe_async;

mod remap;
mod sector_linear;
mod xdvdfs;

pub use remap::*;
pub use sector_linear::*;
pub use xdvdfs::*;

#[cfg(not(target_family = "wasm"))]
mod stdfs;

#[cfg(not(target_family = "wasm"))]
pub use stdfs::*;

#[derive(Copy, Clone, Debug)]
pub enum FileType {
    File,
    Directory,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PathVec {
    components: Vec<String>,
}

type PathVecIter<'a> = Iter<'a, String>;

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub file_type: FileType,
    pub len: u64,
}

#[derive(Clone, Debug)]
pub struct DirectoryTreeEntry {
    pub dir: PathVec,
    pub listing: Vec<FileEntry>,
}

impl PathVec {
    pub fn as_path_buf(&self, prefix: &Path) -> PathBuf {
        let suffix = PathBuf::from_iter(self.components.iter());
        prefix.join(suffix)
    }

    pub fn is_root(&self) -> bool {
        self.components.is_empty()
    }

    pub fn iter(&self) -> PathVecIter {
        self.components.iter()
    }

    pub fn from_base(prefix: &Self, suffix: &str) -> Self {
        let mut path = prefix.clone();
        path.components.push(suffix.to_owned());
        path
    }

    pub fn as_string(&self) -> String {
        format!("/{}", self.components.join("/"))
    }

    pub fn base(&self) -> PathVec {
        PathVec {
            components: self.components[0..self.components.len() - 1].to_vec(),
        }
    }

    pub fn suffix(&self, prefix: &Self) -> Self {
        let mut components = Vec::new();
        let mut i1 = self.iter();
        let mut i2 = prefix.iter();

        loop {
            let c1 = i1.next();
            let c2 = i2.next();

            if let Some(component) = c1 {
                if let Some(component2) = c2 {
                    assert_eq!(component, component2);
                } else {
                    components.push(component.clone());
                }
            } else {
                return Self { components };
            }
        }
    }
}

impl<'a> FromIterator<&'a str> for PathVec {
    fn from_iter<T: IntoIterator<Item = &'a str>>(iter: T) -> Self {
        let components = iter
            .into_iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_owned())
            .collect();
        Self { components }
    }
}

#[maybe_async]
pub trait Filesystem<RawHandle: BlockDeviceWrite<RHError>, E, RHError: Into<E> = E>:
    Send + Sync
{
    /// Read a directory, and return a list of entries within it
    async fn read_dir(&mut self, path: &PathVec) -> Result<Vec<FileEntry>, E>;

    /// Copy the entire contents of file `src` into `dest` at the specified offset
    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut RawHandle,
        offset: u64,
        size: u64,
    ) -> Result<u64, E>;

    /// Copy the contents of file `src` into `buf` at the specified offset
    /// Not required for normal usage
    async fn copy_file_buf(
        &mut self,
        _src: &PathVec,
        _buf: &mut [u8],
        _offset: u64,
    ) -> Result<u64, E>;

    /// Display a filesystem path as a String
    fn path_to_string(&self, path: &PathVec) -> String {
        path.as_string()
    }
}

#[maybe_async]
impl<E: Send + Sync, R: BlockDeviceWrite<E>> Filesystem<R, E> for Box<dyn Filesystem<R, E>> {
    async fn read_dir(&mut self, path: &PathVec) -> Result<Vec<FileEntry>, E> {
        self.as_mut().read_dir(path).await
    }

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut R,
        offset: u64,
        size: u64,
    ) -> Result<u64, E> {
        self.as_mut().copy_file_in(src, dest, offset, size).await
    }

    async fn copy_file_buf(
        &mut self,
        src: &PathVec,
        buf: &mut [u8],
        offset: u64,
    ) -> Result<u64, E> {
        self.as_mut().copy_file_buf(src, buf, offset).await
    }

    fn path_to_string(&self, path: &PathVec) -> String {
        self.as_ref().path_to_string(path)
    }
}

#[derive(Clone, Debug)]
struct PathPrefixTree<T> {
    children: [Option<Box<PathPrefixTree<T>>>; 256],
    pub record: Option<(T, Box<PathPrefixTree<T>>)>,
}

struct PPTIter<'a, T> {
    queue: Vec<(String, &'a PathPrefixTree<T>)>,
}

impl<'a, T> Iterator for PPTIter<'a, T> {
    type Item = (String, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        use alloc::borrow::ToOwned;

        // Expand until we find a node with a record
        while let Some(subtree) = self.queue.pop() {
            let (name, node) = &subtree;
            for (ch, child) in node.children.iter().enumerate() {
                if let Some(child) = child {
                    let mut name = name.to_owned();
                    name.push(ch as u8 as char);
                    self.queue.push((name, child));
                }
            }

            if let Some(record) = &node.record {
                return Some((name.to_owned(), &record.0));
            }
        }

        None
    }
}

impl<T> Default for PathPrefixTree<T> {
    fn default() -> Self {
        Self {
            children: [const { None }; 256],
            record: None,
        }
    }
}

impl<T> PathPrefixTree<T> {
    /// Looks up a node, only descending into subdirs if the path is not consumed
    pub fn lookup_node(&self, path: &PathVec) -> Option<&Self> {
        let mut node = self;

        let mut component_iter = path.iter().peekable();
        while let Some(component) = component_iter.next() {
            for ch in component.chars() {
                let next = &node.children[ch as usize];
                node = next.as_ref()?;
            }

            if component_iter.peek().is_some() {
                let record = &node.record;
                let (_, subtree) = record.as_ref()?;
                node = subtree;
            }
        }

        Some(node)
    }

    /// Looks up a node, only descending into subdirs if the path is not consumed
    pub fn lookup_node_mut(&mut self, path: &PathVec) -> Option<&mut Self> {
        let mut node = self;

        let mut component_iter = path.iter().peekable();
        while let Some(component) = component_iter.next() {
            for ch in component.chars() {
                let next = &mut node.children[ch as usize];
                node = next.as_mut()?;
            }

            if component_iter.peek().is_some() {
                let record = &mut node.record;
                let (_, subtree) = record.as_mut()?;
                node = subtree;
            }
        }

        Some(node)
    }

    /// Looks up a subdir, returning its subtree
    pub fn lookup_subdir(&self, path: &PathVec) -> Option<&Self> {
        let mut node = self;

        for component in path.iter() {
            for ch in component.chars() {
                let next = &node.children[ch as usize];
                node = next.as_ref()?;
            }

            let record = &node.record;
            let (_, subtree) = record.as_ref()?;
            node = subtree;
        }

        Some(node)
    }

    pub fn insert_tail(&mut self, tail: &str, val: T) -> &mut Self {
        let mut node = self;

        for ch in tail.chars() {
            let next = &mut node.children[ch as usize];
            node = next.get_or_insert_with(|| Box::new(Self::default()));
        }

        node.record
            .get_or_insert_with(|| (val, Box::new(Self::default())))
            .1
            .as_mut()
    }

    pub fn get(&self, path: &PathVec) -> Option<&T> {
        let node = self.lookup_node(path)?;
        node.record.as_ref().map(|v| &v.0)
    }

    pub fn iter(&self) -> PPTIter<'_, T> {
        PPTIter {
            queue: alloc::vec![(String::new(), self)],
        }
    }
}
