use core::fmt::{Debug, Display};

#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Error<E> {
    IOError(E),
    SizeOutOfBounds(u64, u32),
    SerializationFailed,
    InvalidVolume,
    DirectoryEmpty,
    DoesNotExist,
    StringEncodingError,
    NoDirent,
    IsNotDirectory,
    NameTooLong,
    InvalidFileName,
    TooManyDirectoryEntries,
    FileTooLarge,
}

impl<E> Error<E> {
    fn to_str(&self) -> &str {
        match self {
            Self::IOError(_) => "IOError",
            Self::SizeOutOfBounds(_, _) => "File size out of bounds",
            Self::SerializationFailed => "Serialization failed",
            Self::InvalidVolume => "Not an XDVDFS volume",
            Self::DirectoryEmpty => "Directory is empty",
            Self::DoesNotExist => "Entry does not exist",
            Self::StringEncodingError => "Cannot decode string into UTF-8",
            Self::NoDirent => "Path exists, but dirent does not (likely root)",
            Self::IsNotDirectory => "Expected directory, found file",
            Self::NameTooLong => "File name is too long",
            Self::InvalidFileName => "Invalid file name",
            Self::TooManyDirectoryEntries => "Too many entries in directory",
            Self::FileTooLarge => "File is too large",
        }
    }
}

impl<E: Display> Display for Error<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IOError(ref e) => {
                f.write_str("IOError: ")?;
                e.fmt(f)
            }
            Self::SizeOutOfBounds(offset, size) => f.write_str(
                alloc::format!("File size out of bounds: {offset} for size {size}").as_str(),
            ),
            other => f.write_str(other.to_str()),
        }
    }
}

impl<E: Display> From<Error<E>> for alloc::string::String {
    fn from(value: Error<E>) -> Self {
        alloc::format!("{value}")
    }
}

impl<E: Debug + Display> core::error::Error for Error<E> {}

impl<E> From<E> for Error<E> {
    fn from(value: E) -> Self {
        Self::IOError(value)
    }
}
