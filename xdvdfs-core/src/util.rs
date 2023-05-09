use core::fmt::{Debug, Display};

#[non_exhaustive]
#[derive(Debug)]
pub enum Error<E> {
    IOError(E),
    SizeOutOfBounds,
    SerializationFailed(bincode::Error),
    InvalidVolume,
    DirectoryEmpty,
    DoesNotExist,
    StringEncodingError,
    NoDirent,
    IsNotDirectory,
    NameTooLong,
    InvalidFileName,
    TooManyDirectoryEntries,
    Unexpected(alloc::string::String),
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
            Self::StringEncodingError => "Cannot decode string into UTF-8",
            Self::NoDirent => "Path exists, but dirent does not (likely root)",
            Self::IsNotDirectory => "Expected directory, found file",
            Self::NameTooLong => "File name is too long",
            Self::InvalidFileName => "Invalid file name",
            Self::TooManyDirectoryEntries => "Too many entries in directory",
            Self::Unexpected(_) => "Unexpected error",
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
            Self::SerializationFailed(ref e) => {
                f.write_str("Serialization failed: ")?;
                Display::fmt(e, f)
            }
            Self::Unexpected(s) => {
                f.write_str("Unexpected error: ")?;
                f.write_str(s)
            }
            other => f.write_str(other.to_str()),
        }
    }
}

#[cfg(feature = "std")]
impl<E: Debug + Display> std::error::Error for Error<E> {}

impl<E> From<E> for Error<E> {
    fn from(value: E) -> Self {
        Self::IOError(value)
    }
}

pub(crate) trait ToUnexpectedError<T, E> {
    fn or_unexpected(self) -> Result<T, Error<E>>;
}

impl<T, V: Debug, E> ToUnexpectedError<T, E> for Result<T, V> {
    fn or_unexpected(self) -> Result<T, Error<E>> {
        // FIXME: Add feature to disable this and just use an empty string or something
        // to avoid pulling in formatting code
        self.map_err(|e| Error::Unexpected(alloc::format!("{:?}", e)))
    }
}

pub fn cmp_ignore_case_utf8(a: &str, b: &str) -> core::cmp::Ordering {
    use core::cmp::Ordering;
    use itertools::{EitherOrBoth, Itertools};

    a.chars()
        .map(|c| c.to_ascii_uppercase())
        .zip_longest(b.chars().map(|c| c.to_ascii_uppercase()))
        .map(|ab| match ab {
            EitherOrBoth::Left(_) => Ordering::Greater,
            EitherOrBoth::Right(_) => Ordering::Less,
            EitherOrBoth::Both(a, b) => a.cmp(&b),
        })
        .find(|&ordering| ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

#[cfg(test)]
mod test {
    use crate::util::cmp_ignore_case_utf8;

    #[test]
    fn test_str_ignore_case_alpha() {
        let mut strings = ["asdf", "GHJK", "bsdf", "AAAA"];
        strings.sort_by(|a, b| cmp_ignore_case_utf8(a, b));

        assert_eq!(strings, ["AAAA", "asdf", "bsdf", "GHJK"]);
    }

    /// Edge case: underscore should be ordered greater than alphanumerics
    #[test]
    fn test_str_ignore_case_special() {
        let mut strings = ["a_b", "abb"];
        strings.sort_by(|a, b| cmp_ignore_case_utf8(a, b));

        assert_eq!(strings, ["abb", "a_b"]);
    }

    #[test]
    fn test_str_ignore_case_ordering() {
        let mut strings = [
            "NFL.png",
            "NFLFever.jpg",
            "NFLFever-noESRB.jpg",
            "NFLFevertrial.xbe",
            "NFL-noESRB.png",
            "art",
        ];
        strings.sort_by(|a, b| cmp_ignore_case_utf8(a, b));

        assert_eq!(
            strings,
            [
                "art",
                "NFL-noESRB.png",
                "NFL.png",
                "NFLFever-noESRB.jpg",
                "NFLFever.jpg",
                "NFLFevertrial.xbe",
            ]
        );
    }
}
