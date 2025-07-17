use core::error::Error;
use core::fmt::{Debug, Display};

mod avl;
pub mod dirtab;
pub mod fs;
pub mod img;
pub mod sector;

/// Container for serialization errors
/// Supplies implementations of Eq, PartialEq for use in FileStructureError
#[derive(Debug)]
pub struct SerializationError(bincode::Error);

impl From<bincode::Error> for SerializationError {
    fn from(value: bincode::Error) -> Self {
        Self(value)
    }
}

impl PartialEq for SerializationError {
    fn eq(&self, _other: &Self) -> bool {
        // Treat all serialization errors as equal
        // bincode::Error does not implement Eq or PartialEq
        true
    }
}

impl Eq for SerializationError {}

/// Contains variants of WriteError that are not specific to
/// the block device or filesystem. This allows us to pass errors
/// around the write module without having to carry around the generics.
#[derive(Debug, PartialEq, Eq)]
pub enum FileStructureError {
    SerializationError(SerializationError),
    StringEncodingError,
    FileNameTooLong,
    DuplicateFileName,
    TooManyDirectoryEntries,
    FileTooLarge,
}

#[derive(Debug)]
pub enum WriteError<BlockDevError, FSHierarchyError, FSCopierError> {
    BlockDeviceError(BlockDevError),
    FilesystemHierarchyError(FSHierarchyError),
    FilesystemCopierError(FSCopierError),
    InvalidFileStructureError(FileStructureError),
}

impl<BDE, FSHE, FSCE> From<FileStructureError> for WriteError<BDE, FSHE, FSCE> {
    fn from(value: FileStructureError) -> Self {
        Self::InvalidFileStructureError(value)
    }
}

impl FileStructureError {
    fn to_str(&self) -> &str {
        match self {
            Self::SerializationError(_) => "Serialization failed",
            Self::StringEncodingError => "Cannot decode string into UTF-8",
            Self::FileNameTooLong => "File name is too long",
            Self::DuplicateFileName => "Duplicate file name",
            Self::TooManyDirectoryEntries => "Too many entries in directory",
            Self::FileTooLarge => "File is too large",
        }
    }
}

impl Display for FileStructureError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::SerializationError(ref e) => {
                f.write_str("Serialization failed: ")?;
                Display::fmt(&e.0, f)
            }
            // FIXME: Encode context for other errors
            other => f.write_str(other.to_str()),
        }
    }
}

impl Error for FileStructureError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SerializationError(ref e) => Some(&e.0),
            _ => None,
        }
    }
}

impl<BDE, FSHE, FSCE> WriteError<BDE, FSHE, FSCE> {
    fn to_str(&self) -> &str {
        match self {
            Self::BlockDeviceError(_) => "Block device write failed",
            Self::FilesystemHierarchyError(_) => "Filesystem hierarchy query failed",
            Self::FilesystemCopierError(_) => "Filesystem to block device copy failed",
            Self::InvalidFileStructureError(_) => "Failed to create XDVDFS filesystem",
        }
    }
}

impl<BDE: Display, FSHE: Display, FSCE: Display> Display for WriteError<BDE, FSHE, FSCE> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.to_str())?;
        f.write_str(": ")?;

        match self {
            Self::BlockDeviceError(ref e) => Display::fmt(e, f),
            Self::FilesystemHierarchyError(ref e) => Display::fmt(e, f),
            Self::FilesystemCopierError(ref e) => Display::fmt(e, f),
            Self::InvalidFileStructureError(ref e) => Display::fmt(e, f),
        }
    }
}

impl<BDE, FSHE, FSCE> Error for WriteError<BDE, FSHE, FSCE>
where
    BDE: Display + Debug + Error + 'static,
    FSHE: Display + Debug + Error + 'static,
    FSCE: Display + Debug + Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::BlockDeviceError(ref e) => Some(e),
            Self::FilesystemHierarchyError(ref e) => Some(e),
            Self::FilesystemCopierError(ref e) => Some(e),
            Self::InvalidFileStructureError(ref e) => Some(e),
        }
    }
}
