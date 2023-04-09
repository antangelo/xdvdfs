use core::fmt::Display;

#[non_exhaustive]
#[derive(Debug)]
pub enum Error<E> {
    IOError(E),
    SizeOutOfBounds,
    SerializationFailed(bincode::Error),
    InvalidVolume,
    DirectoryEmpty,
    DoesNotExist,
    UTFError(core::str::Utf8Error),
    NoDirent,
    IsNotDirectory,
}

impl<E> Error<E> {
    fn to_str(&self) -> &str {
        match self {
            Self::IOError(_) => "IOError",
            Self::SizeOutOfBounds => "File size out of bounds",
            Self::SerializationFailed(_) => "Serialization failed",
            Self::InvalidVolume => "Not an XDVDFS volume",
            Self::DirectoryEmpty => "Directory is empty",
            Self::DoesNotExist => "Entry does not exist",
            Self::UTFError(_) => "UTF Error",
            Self::NoDirent => "Path exists, but dirent does not (likely root)",
            Self::IsNotDirectory => "Expected directory, found file",
        }
    }
}

impl<E: Display> Display for Error<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IOError(ref e) => {
                f.write_str("IOError: ")?;
                e.fmt(f)?;
            }
            Self::SerializationFailed(ref e) => {
                f.write_str("Serialization failed: ")?;
                e.fmt(f)?;
            }
            Self::UTFError(e) => {
                f.write_str("UTF Error: ")?;
                e.fmt(f)?;
            }
            other => f.write_str(other.to_str())?,
        }

        Ok(())
    }
}

pub fn cmp_ignore_case_utf8(a: &str, b: &str) -> core::cmp::Ordering {
    use core::cmp::Ordering;
    use itertools::{EitherOrBoth, Itertools};

    a.chars()
        .flat_map(char::to_lowercase)
        .zip_longest(b.chars().flat_map(char::to_lowercase))
        .map(|ab| match ab {
            EitherOrBoth::Left(_) => Ordering::Greater,
            EitherOrBoth::Right(_) => Ordering::Less,
            EitherOrBoth::Both(a, b) => a.cmp(&b),
        })
        .find(|&ordering| ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}
