use core::fmt::Display;

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;
use maybe_async::maybe_async;

#[cfg(not(feature = "sync"))]
use alloc::boxed::Box;

use crate::blockdev::BlockDeviceWrite;

use super::{FileEntry, FileType, FilesystemCopier, FilesystemHierarchy, PathPrefixTree, PathVec};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RemapOverlayConfig {
    // host path regex -> image path rewrite
    pub map_rules: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
struct MapEntry {
    host_path: PathVec,
    host_entry: FileEntry,
    is_prefix_directory: bool,
}

#[derive(Debug)]
pub enum InvalidRewriteSubstitutionKind {
    NonDigitCharacter,
    UnclosedBrace,
}

#[derive(Debug)]
pub enum RemapOverlayFilesystemBuildingError<E> {
    GlobBuildingError(wax::BuildError),
    InvalidRewriteSubstitution(usize, String, InvalidRewriteSubstitutionKind),
    FilesystemError(E),
}

impl<E: Display> RemapOverlayFilesystemBuildingError<E> {
    pub fn as_string(&self) -> String {
        match self {
            Self::FilesystemError(e) => alloc::format!("error in underlying filesystem: {}", e),
            Self::GlobBuildingError(e) => alloc::format!("failed to build glob pattern: {}", e),
            Self::InvalidRewriteSubstitution(idx, rewrite, kind) => alloc::format!(
                "invalid rewrite substitution \"{}\" (at {}): {}",
                rewrite,
                idx,
                match kind {
                    InvalidRewriteSubstitutionKind::NonDigitCharacter => "expected digit character",
                    InvalidRewriteSubstitutionKind::UnclosedBrace => "unclosed brace",
                }
            ),
        }
    }
}

impl<E: Display> Display for RemapOverlayFilesystemBuildingError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_string().as_str())
    }
}

impl<E: Display + core::fmt::Debug> std::error::Error for RemapOverlayFilesystemBuildingError<E> {}

#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum RemapOverlayError<E> {
    NoSuchFile(String),
    UnderlyingError(E),
}

impl<E> From<E> for RemapOverlayError<E> {
    fn from(value: E) -> Self {
        Self::UnderlyingError(value)
    }
}

impl<E: Display> RemapOverlayError<E> {
    pub fn as_string(&self) -> String {
        match self {
            Self::NoSuchFile(image) => alloc::format!("no host mapping for image path: {}", image),
            Self::UnderlyingError(e) => alloc::format!("error in underlying filesystem: {}", e),
        }
    }
}

impl<E: Display> Display for RemapOverlayError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_string().as_str())
    }
}

impl<E: Display + core::fmt::Debug> std::error::Error for RemapOverlayError<E> {}

#[derive(Clone, Debug)]
pub struct RemapOverlayFilesystem<FS> {
    img_to_host: PathPrefixTree<MapEntry>,
    fs: FS,
}

impl<E, FS> RemapOverlayFilesystem<FS>
where
    E: Into<RemapOverlayError<E>> + Send + Sync,
    FS: FilesystemHierarchy<Error = E>,
{
    fn find_match_indices(
        rewrite: &str,
    ) -> Result<Vec<usize>, RemapOverlayFilesystemBuildingError<E>> {
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
    pub async fn new(
        mut fs: FS,
        cfg: RemapOverlayConfig,
    ) -> Result<Self, RemapOverlayFilesystemBuildingError<E>> {
        use wax::Pattern;

        let glob_keys: Result<Vec<wax::Glob>, _> = cfg
            .map_rules
            .iter()
            .map(|(from, _)| wax::Glob::new(from.trim_start_matches('!')))
            .collect();
        let glob_keys =
            glob_keys.map_err(|e| RemapOverlayFilesystemBuildingError::GlobBuildingError(e))?;
        let all_globs = wax::any(glob_keys.clone().into_iter())
            .map_err(|e| RemapOverlayFilesystemBuildingError::GlobBuildingError(e))?;

        // Walk the host and store any paths that match the mapping rules
        let mut host_dirs = alloc::vec![(PathVec::default(), None)];
        let mut matches: Vec<(PathVec, FileEntry, PathVec)> = Vec::new();
        while let Some((dir, parent_match_prefix)) = host_dirs.pop() {
            let listing = fs
                .read_dir(&dir)
                .await
                .map_err(|e| RemapOverlayFilesystemBuildingError::FilesystemError(e))?;
            for entry in listing.iter() {
                let path = PathVec::from_base(&dir, &entry.name);
                let match_prefix = if all_globs.is_match(path.as_string().trim_start_matches('/')) {
                    Some(path.clone())
                } else if parent_match_prefix.is_some() {
                    parent_match_prefix.clone()
                } else {
                    None
                };

                if let FileType::Directory = entry.file_type {
                    host_dirs.push((PathVec::from_base(&dir, &entry.name), match_prefix.clone()));
                }

                if let Some(prefix) = match_prefix {
                    matches.push((path.clone(), entry.clone(), prefix));
                }
            }
        }

        let mut img_to_host = PathPrefixTree::default();
        for (path, entry, prefix) in matches {
            let suffix = path.suffix(&prefix);
            let mut rewritten_path: Option<PathVec> = None;

            for (idx, glob) in glob_keys.iter().enumerate() {
                let path_str = prefix.as_string();

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
                    rewrite =
                        alloc::format!("{}{}", rewrite.trim_end_matches('/'), suffix.as_string());
                }

                let rewrite = PathVec::from_iter(
                    rewrite
                        .trim_start_matches('.')
                        .trim_start_matches('/')
                        .split('/'),
                );
                rewritten_path = Some(rewrite);
            }

            // If we have a valid rewritten path, we can insert it into the new filesystem
            if let Some(rewrite) = rewritten_path {
                let mut rewrite = rewrite.iter().peekable();
                let mut node = &mut img_to_host;

                while let Some(component) = rewrite.next() {
                    let is_prefix_directory = rewrite.peek().is_some();
                    let entry = if !is_prefix_directory {
                        entry.clone()
                    } else {
                        FileEntry {
                            name: component.to_owned(),
                            file_type: FileType::Directory,
                            len: 0,
                        }
                    };

                    node = node.insert_tail(
                        component,
                        MapEntry {
                            host_entry: entry,
                            host_path: path.clone(),
                            is_prefix_directory,
                        },
                    );
                }
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
                let path = PathVec::from_base(&path, &name);

                if !entry.is_prefix_directory {
                    output.push((entry.host_path.clone(), path.clone()));
                }

                if let FileType::Directory = entry.host_entry.file_type {
                    queue.push(path);
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
        path: &PathVec,
    ) -> Result<Vec<FileEntry>, RemapOverlayError<F::Error>> {
        let dir = self
            .img_to_host
            .lookup_subdir(path)
            .ok_or_else(|| RemapOverlayError::NoSuchFile(path.as_string()))?;
        let entries: Vec<FileEntry> = dir
            .iter()
            .map(|(name, entry)| FileEntry {
                name,
                file_type: entry.host_entry.file_type,
                len: entry.host_entry.len,
            })
            .collect();

        Ok(entries)
    }
}

#[maybe_async]
impl<BDW, E, FS> FilesystemCopier<BDW> for RemapOverlayFilesystem<FS>
where
    E: Into<RemapOverlayError<E>> + Send + Sync,
    BDW: BlockDeviceWrite,
    FS: FilesystemCopier<BDW, Error = E>,
{
    type Error = RemapOverlayError<E>;

    async fn copy_file_in(
        &mut self,
        src: &PathVec,
        dest: &mut BDW,
        offset: u64,
        size: u64,
    ) -> Result<u64, RemapOverlayError<E>> {
        let entry = self
            .img_to_host
            .get(src)
            .ok_or_else(|| RemapOverlayError::NoSuchFile(src.as_string()))?;
        self.fs
            .copy_file_in(&entry.host_path, dest, offset, size)
            .await
            .map_err(|e| e.into())
    }
}
