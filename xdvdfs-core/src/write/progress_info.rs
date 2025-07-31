use super::fs::{PathCow, PathVec};

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProgressInfo<'a> {
    DiscoveredDirectory(usize),
    FileCount(usize),
    DirCount(usize),
    DirAdded(PathCow<'a>, u64),
    FileAdded(PathCow<'a>, u64),
    FinishedCopyingImageData,
    FinishedPacking,
}

#[non_exhaustive]
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OwnedProgressInfo {
    DiscoveredDirectory(usize),
    FileCount(usize),
    DirCount(usize),
    DirAdded(PathVec, u64),
    FileAdded(PathVec, u64),
    FinishedCopyingImageData,
    FinishedPacking,
}

impl ProgressInfo<'_> {
    pub fn to_owned(self) -> OwnedProgressInfo {
        match self {
            Self::DiscoveredDirectory(len) => OwnedProgressInfo::DiscoveredDirectory(len),
            Self::FileCount(count) => OwnedProgressInfo::FileCount(count),
            Self::DirCount(count) => OwnedProgressInfo::DirCount(count),
            Self::DirAdded(path, size) => OwnedProgressInfo::DirAdded(path.to_owned(), size),
            Self::FileAdded(path, size) => OwnedProgressInfo::FileAdded(path.to_owned(), size),
            Self::FinishedCopyingImageData => OwnedProgressInfo::FinishedCopyingImageData,
            Self::FinishedPacking => OwnedProgressInfo::FinishedPacking,
        }
    }
}
