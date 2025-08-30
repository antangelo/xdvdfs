use core::fmt::Display;

use alloc::borrow::ToOwned;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use maybe_async::maybe_async;
use thiserror::Error;

#[cfg(not(feature = "sync"))]
use alloc::boxed::Box;
use wax::{Glob, Pattern};

use crate::{blockdev::BlockDeviceWrite, write::fs::PathRef};

use super::{FileEntry, FileType, FilesystemCopier, FilesystemHierarchy, PathPrefixTree, PathVec};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RemapOverlayConfig {
    // host path regex -> image path rewrite
    pub map_rules: Vec<(String, String)>,
}

/*
#[derive(Clone, Debug)]
struct MapEntry {
    host_path: PathVec,
    host_entry: FileEntry,
    is_prefix_directory: bool,
}
*/

#[derive(Clone, Debug, Default)]
enum MapEntry {
    #[default]
    GeneratedDir,
    HostEntry {
        host_path: PathVec,
        host_entry: FileEntry,
    },
}

impl MapEntry {
    fn as_file_entry(&self, name: String) -> FileEntry {
        match self {
            Self::GeneratedDir => FileEntry {
                name,
                file_type: FileType::Directory,
                len: 0,
            },
            Self::HostEntry { host_entry, .. } => FileEntry {
                name,
                file_type: host_entry.file_type,
                len: host_entry.len,
            },
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum InvalidRewriteSubstitutionKind {
    NonDigitCharacter,
    UnclosedBrace,
}

impl Display for InvalidRewriteSubstitutionKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            InvalidRewriteSubstitutionKind::NonDigitCharacter => {
                write!(f, "expected digit character")
            }
            InvalidRewriteSubstitutionKind::UnclosedBrace => write!(f, "unclosed brace"),
        }
    }
}

#[derive(Error, Debug)]
pub enum RemapOverlayFilesystemBuildingError<E> {
    #[error("failed to build glob pattern: {0}")]
    GlobBuildingError(#[from] wax::BuildError),
    #[error("invalid rewrite substitution \"{1}\" (at {0}): {2}")]
    InvalidRewriteSubstitution(usize, String, InvalidRewriteSubstitutionKind),
    #[error("error in underlying filesystem: {0}")]
    FilesystemError(#[source] E),
}

#[non_exhaustive]
#[derive(Error, Clone, Debug)]
pub enum RemapOverlayError<E> {
    #[error("no host mapping for image path \"{0}\"")]
    NoSuchFile(String),
    #[error("error in underlying filesystem: {0}")]
    UnderlyingError(#[from] E),
}

#[derive(Clone, Debug)]
pub struct RemapOverlayFilesystem<FS> {
    img_to_host: PathPrefixTree<MapEntry>,
    fs: FS,
}

impl<FS> RemapOverlayFilesystem<FS>
where
    FS: FilesystemHierarchy,
{
    fn find_match_indices(
        rewrite: &str,
    ) -> Result<Vec<usize>, RemapOverlayFilesystemBuildingError<FS::Error>> {
        let mut match_indices: Vec<usize> = Vec::new();
        let mut match_index = 0;
        let mut matching = false;

        for (idx, c) in rewrite.chars().enumerate() {
            // TODO: Allow these to be escaped. Are {} characters valid anyway?
            if c == '{' {
                matching = true;
                continue;
            }

            if !matching {
                continue;
            }

            if c == '}' {
                matching = false;
                match_indices.push(match_index);
                match_index = 0;
                continue;
            }

            if let Some(digit) = c.to_digit(10) {
                match_index *= 10;
                match_index += digit as usize;
                continue;
            }

            return Err(
                RemapOverlayFilesystemBuildingError::InvalidRewriteSubstitution(
                    idx,
                    rewrite.to_owned(),
                    InvalidRewriteSubstitutionKind::NonDigitCharacter,
                ),
            );
        }

        if matching {
            return Err(
                RemapOverlayFilesystemBuildingError::InvalidRewriteSubstitution(
                    rewrite.len() - 1,
                    rewrite.to_owned(),
                    InvalidRewriteSubstitutionKind::UnclosedBrace,
                ),
            );
        }

        Ok(match_indices)
    }

    #[maybe_async]
    async fn get_host_paths_matching_globs(
        fs: &mut FS,
        glob_keys: &Vec<Glob<'_>>,
    ) -> Result<Vec<(PathVec, FileEntry, PathVec)>, RemapOverlayFilesystemBuildingError<FS::Error>> {
        let all_globs = wax::any(glob_keys.clone().into_iter())?;

        let mut host_dir_stack = alloc::vec![(PathVec::default(), None)];
        let mut matches: Vec<(PathVec, FileEntry, PathVec)> = Vec::new();

        while let Some((dir, parent_match_prefix)) = host_dir_stack.pop() {
            let listing = fs
                .read_dir(dir.as_path_ref())
                .await
                .map_err(RemapOverlayFilesystemBuildingError::FilesystemError)?;
            for entry in listing.iter() {
                let path = PathVec::from_base(dir.clone(), &entry.name);
                let match_prefix = if all_globs.is_match(path.to_string().trim_start_matches('/')) {
                    Some(path.clone())
                } else if parent_match_prefix.is_some() {
                    parent_match_prefix.clone()
                } else {
                    None
                };

                if let FileType::Directory = entry.file_type {
                    host_dir_stack.push((
                        PathVec::from_base(dir.clone(), &entry.name),
                        match_prefix.clone(),
                    ));
                }

                if let Some(prefix) = match_prefix {
                    matches.push((path.clone(), entry.clone(), prefix));
                }
            }
        }

        Ok(matches)
    }

    fn get_rewritten_path(
        cfg: &RemapOverlayConfig,
        glob_keys: &Vec<Glob<'_>>,
        path: &PathVec,
        prefix: &PathVec,
    ) -> Result<Option<PathVec>, RemapOverlayFilesystemBuildingError<FS::Error>> {
        let suffix = path.suffix(&prefix);
        let mut rewritten_path: Option<PathVec> = None;

        for (idx, glob) in glob_keys.iter().enumerate() {
            let path_str = prefix.to_string();

            // Find which specific glob was matched by this path
            let cand_path = wax::CandidatePath::from(path_str.trim_start_matches('/'));
            let matched = glob.matched(&cand_path);
            let Some(matched) = matched else {
                continue;
            };

            // Negating patterns erase any rewritten_path we have come across
            if cfg.map_rules[idx].0.starts_with('!') {
                rewritten_path = None;
                continue;
            }

            // Prefer previously matched patterns, if any
            if rewritten_path.is_some() {
                continue;
            }

            let mut rewrite = cfg.map_rules[idx].1.clone();
            let match_indices = Self::find_match_indices(&rewrite)?;
            for index in match_indices {
                let replace = matched.get(index).unwrap_or("");
                rewrite = rewrite.replace(&alloc::format!("{{{index}}}"), replace);
            }

            // If this path matched a prefix (e.g. the rule "bin") and has a suffix (e.g.
            // "/default.xbe"), then we need to re-add the suffix to the rewritten prefix
            if !suffix.is_root() {
                let rewritten_prefix = rewrite.trim_end_matches('/');
                rewrite = alloc::format!("{rewritten_prefix}{suffix}");
            }

            let rewrite = PathVec::from_iter(
                rewrite
                .trim_start_matches('.')
                .trim_start_matches('/')
                .split('/'),
            );
            rewritten_path = Some(rewrite);
        }

        Ok(rewritten_path)
    }

    #[maybe_async]
    pub async fn new(
        mut fs: FS,
        cfg: RemapOverlayConfig,
    ) -> Result<Self, RemapOverlayFilesystemBuildingError<FS::Error>> {
        let glob_keys: Result<Vec<wax::Glob>, _> = cfg
            .map_rules
            .iter()
            .map(|(from, _)| wax::Glob::new(from.trim_start_matches('!')))
            .collect();
        let glob_keys = glob_keys?;
        
        let matches = Self::get_host_paths_matching_globs(&mut fs, &glob_keys).await?;

        let mut img_to_host = PathPrefixTree::default();
        for (path, entry, prefix) in matches {
            let rewritten_path = Self::get_rewritten_path(&cfg, &glob_keys, &path, &prefix)?;

            // If we have a valid rewritten path, we can insert it into the new filesystem
            if let Some(rewrite) = rewritten_path {
                // Rewrites to root are merged with the root dirent,
                // and children are rewritten separately.
                if rewrite.is_root() {
                    continue;
                }

                img_to_host.insert_path(&rewrite, MapEntry::HostEntry {
                    host_path: path,
                    host_entry: entry,
                });
            }
        }

        Ok(Self { img_to_host, fs })
    }

    pub fn dump(&self) -> Vec<(PathVec, PathVec)> {
        let mut queue = alloc::vec![PathVec::default()];
        let mut output: Vec<(PathVec, PathVec)> = Vec::new();

        while let Some(path) = queue.pop() {
            let listing = self
                .img_to_host
                .lookup_subdir(&path)
                .expect("failed trie lookup for vfs directory");
            for (name, entry) in listing.iter() {
                let path = PathVec::from_base(path.clone(), &name);
                match entry {
                    MapEntry::GeneratedDir => {
                        queue.push(path);
                    },
                    MapEntry::HostEntry { host_path, host_entry } => {
                        output.push((host_path.clone(), path.clone()));

                        if let FileType::Directory = host_entry.file_type {
                            queue.push(path);
                        }
                    },
                }
            }
        }

        output
    }
}

#[maybe_async]
impl<F> FilesystemHierarchy for RemapOverlayFilesystem<F>
where
    F: FilesystemHierarchy,
{
    type Error = RemapOverlayError<F::Error>;

    async fn read_dir(
        &mut self,
        path: PathRef<'_>,
    ) -> Result<Vec<FileEntry>, RemapOverlayError<F::Error>> {
        let dir = self
            .img_to_host
            .lookup_subdir(path)
            .ok_or_else(|| RemapOverlayError::NoSuchFile(path.to_string()))?;
        let entries: Vec<FileEntry> = dir
            .iter()
            .map(|(name, entry)| entry.as_file_entry(name))
            .collect();

        Ok(entries)
    }

    async fn clear_cache(&mut self) -> Result<(), Self::Error> {
        // TODO: Clear underlying FS cache and regenerate
        unimplemented!("cache clearing on a remap filesystem is not implemented")
    }
}

#[maybe_async]
impl<BDW, FS> FilesystemCopier<BDW> for RemapOverlayFilesystem<FS>
where
    BDW: BlockDeviceWrite,
    FS: FilesystemCopier<BDW>,
{
    type Error = RemapOverlayError<FS::Error>;

    async fn copy_file_in(
        &mut self,
        src: PathRef<'_>,
        dest: &mut BDW,
        input_offset: u64,
        output_offset: u64,
        size: u64,
    ) -> Result<u64, Self::Error> {
        let entry = self
            .img_to_host
            .get(src)
            .ok_or_else(|| RemapOverlayError::NoSuchFile(src.to_string()))?;
        let MapEntry::HostEntry { host_path, .. } = entry else {
            return Err(RemapOverlayError::NoSuchFile(src.to_string()));
        };
        self.fs
            .copy_file_in(
                host_path.as_path_ref(),
                dest,
                input_offset,
                output_offset,
                size,
            )
            .await
            .map_err(|e| e.into())
    }
}
