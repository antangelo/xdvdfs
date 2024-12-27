use core::fmt::{Debug, Display};

#[non_exhaustive]
#[derive(Debug)]
pub enum Error<E> {
    IOError(E),
    SizeOutOfBounds(u64, u32),
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
    FileTooLarge,
}

impl<E> Error<E> {
    fn to_str(&self) -> &str {
        match self {
            Self::IOError(_) => "IOError",
            Self::SizeOutOfBounds(_, _) => "File size out of bounds",
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
                alloc::format!("File size out of bounds: {} for size {}", offset, size).as_str(),
            ),
            Self::SerializationFailed(ref e) => {
                f.write_str("Serialization failed: ")?;
                Display::fmt(e, f)
            }
            other => f.write_str(other.to_str()),
        }
    }
}

impl<E: Display> From<Error<E>> for alloc::string::String {
    fn from(value: Error<E>) -> Self {
        alloc::format!("{}", value)
    }
}

#[cfg(feature = "std")]
impl<E: Debug + Display> std::error::Error for Error<E> {}

impl<E> From<E> for Error<E> {
    fn from(value: E) -> Self {
        Self::IOError(value)
    }
}

pub fn cmp_ignore_case_utf8(a: &str, b: &str) -> core::cmp::Ordering {
    use core::cmp::Ordering;

    let mut a_chars = a.chars().map(|c| c.to_ascii_uppercase());
    let mut b_chars = b.chars().map(|c| c.to_ascii_uppercase());

    loop {
        let a_next = a_chars.next();
        let b_next = b_chars.next();
        if a_next.is_none() && b_next.is_none() {
            break Ordering::Equal;
        }

        let a = match a_next {
            Some(a) => a,
            None => break Ordering::Less,
        };

        let b = match b_next {
            Some(b) => b,
            None => break Ordering::Greater,
        };

        match a.cmp(&b) {
            Ordering::Equal => continue,
            cmp => break cmp,
        }
    }
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
