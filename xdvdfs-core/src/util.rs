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
                alloc::format!("File size out of bounds: {offset} for size {size}").as_str(),
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
        alloc::format!("{value}")
    }
}

impl<E: Debug + Display> core::error::Error for Error<E> {}

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

// When building a directory entry table, comparisons between entries
// is done by the name string with upper-case characters. When serializing,
// we no longer need to perform comparisons, but do need the serialized
// name, so we reuse the space.
#[cfg(feature = "write")]
#[derive(Copy, Clone, Debug, Hash)]
enum DirentNameStored {
    WithUpperCase([u8; 256]),
    WithEncoding([u8; 256], usize),
}

#[cfg(feature = "write")]
#[derive(Copy, Clone, Debug, Hash)]
pub struct DirentName<'alloc> {
    name: &'alloc str,
    name_inner: DirentNameStored,
}

#[cfg(feature = "write")]
impl<'alloc> DirentName<'alloc> {
    pub fn new(name: &'alloc str) -> Self {
        let name_len = core::cmp::min(name.len(), 256);
        let name = &name[..name_len];

        let mut name_inner = [0u8; 256];
        name_inner[..name_len].copy_from_slice(name.as_bytes());

        // SAFETY: name_bytes was just copied from a UTF-8 str
        let name_inner_str =
            unsafe { core::str::from_utf8_unchecked_mut(&mut name_inner[..name_len]) };
        name_inner_str.make_ascii_uppercase();

        let name_inner = DirentNameStored::WithUpperCase(name_inner);
        Self { name, name_inner }
    }

    pub fn get_name(&self) -> &str {
        self.name
    }

    pub fn set_encode_name(&mut self) -> Result<u8, crate::write::FileStructureError> {
        use crate::write::FileStructureError;

        if let DirentNameStored::WithEncoding(_, size) = self.name_inner {
            return Ok(size as u8);
        }

        let mut buffer = [0u8; 256];
        let mut encoder = encoding_rs::WINDOWS_1252.new_encoder();

        let (result, bytes_read, bytes_written) =
            encoder.encode_from_utf8_without_replacement(self.name, &mut buffer, true);
        match result {
            encoding_rs::EncoderResult::InputEmpty => {}
            _ => return Err(FileStructureError::StringEncodingError),
        }

        self.name_inner = DirentNameStored::WithEncoding(buffer, bytes_written);

        if bytes_read != self.name.len() {
            Err(FileStructureError::StringEncodingError)
        } else {
            TryInto::<u8>::try_into(bytes_written).map_err(|_| FileStructureError::FileNameTooLong)
        }
    }

    pub fn get_encoded_name(&self) -> &[u8] {
        match &self.name_inner {
            DirentNameStored::WithEncoding(buf, len) => &buf[..*len],
            _ => unreachable!("Must call set_encoded_name before get!"),
        }
    }
}

#[cfg(feature = "write")]
impl PartialEq for DirentName<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == core::cmp::Ordering::Equal
    }
}

#[cfg(feature = "write")]
impl Eq for DirentName<'_> {}

#[cfg(feature = "write")]
impl PartialOrd for DirentName<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(feature = "write")]
impl Ord for DirentName<'_> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        let DirentNameStored::WithUpperCase(self_name) = &self.name_inner else {
            // TODO: Annotate with `cold_path()` once it is stabilized
            return cmp_ignore_case_utf8(self.name, other.name);
        };

        let DirentNameStored::WithUpperCase(other_name) = &other.name_inner else {
            return cmp_ignore_case_utf8(self.name, other.name);
        };

        // SAFETY: WithUpperCase is constructed with a UTF-8 str
        let self_name = unsafe { core::str::from_utf8_unchecked(self_name) };
        let other_name = unsafe { core::str::from_utf8_unchecked(other_name) };

        self_name.cmp(other_name)
    }
}

#[cfg(all(test, feature = "write"))]
fn name_comparator_wrapper(a: &str, b: &str) -> core::cmp::Ordering {
    let a = DirentName::new(a);
    let b = DirentName::new(b);
    a.cmp(&b)
}

macro_rules! case_cmp_test {
    ($name:ident, $cmp:expr) => {
        #[cfg(test)]
        mod $name {
            #[test]
            fn test_str_ignore_case_alpha() {
                let mut strings = ["asdf", "GHJK", "bsdf", "AAAA"];
                strings.sort_by($cmp);

                assert_eq!(strings, ["AAAA", "asdf", "bsdf", "GHJK"]);
            }

            /// Edge case: underscore should be ordered greater than alphanumerics
            #[test]
            fn test_str_ignore_case_special() {
                let mut strings = ["a_b", "abb"];
                strings.sort_by($cmp);

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
                strings.sort_by($cmp);

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
    };
}

case_cmp_test!(test_cmp_fn, |a, b| crate::util::cmp_ignore_case_utf8(a, b));

#[cfg(feature = "write")]
case_cmp_test!(test_name_comparator, |a, b| {
    crate::util::name_comparator_wrapper(a, b)
});
