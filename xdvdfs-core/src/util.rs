use thiserror::Error;

#[non_exhaustive]
#[derive(Error, Debug, Copy, Clone, Eq, PartialEq)]
pub enum Error<E> {
    #[error("io error: {0}")]
    IOError(#[from] E),
    #[error("file size {1} out of bounds for offset {0}")]
    SizeOutOfBounds(u64, u32),
    #[error("serialization failed")]
    SerializationFailed,
    #[error("not an xdvdfs volume")]
    InvalidVolume,
    #[error("directory is empty")]
    DirectoryEmpty,
    #[error("entry does not exist")]
    DoesNotExist,
    #[error("cannot decode string into utf-8")]
    StringEncodingError,
    #[error("path exists, but dirent does not (likely root)")]
    NoDirent,
    #[error("expected directory, found file")]
    IsNotDirectory,
    #[error("file name is too long")]
    NameTooLong,
    #[error("invalid file name")]
    InvalidFileName,
    #[error("too many entries in single directory")]
    TooManyDirectoryEntries,
    #[error("file is too large")]
    FileTooLarge,
}
