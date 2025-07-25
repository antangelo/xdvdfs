use core::fmt::Display;
use core::iter::Map;
use core::slice::Iter;

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;

use super::PathRef;

#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathVec {
    components: Vec<String>,
}

pub type PathVecIter<'a> = Map<Iter<'a, String>, for<'b> fn(&'b String) -> &'b str>;

impl<'a> FromIterator<&'a str> for PathVec {
    fn from_iter<T: IntoIterator<Item = &'a str>>(iter: T) -> Self {
        let components = iter
            .into_iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_owned())
            .collect();
        Self { components }
    }
}

impl<'a> IntoIterator for &'a PathVec {
    type Item = &'a str;
    type IntoIter = PathVecIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> From<&'a str> for PathVec {
    fn from(value: &'a str) -> Self {
        PathVec::from_iter(value.split("/"))
    }
}

impl Display for PathVec {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use core::fmt::Write;

        if self.is_root() {
            return f.write_char('/');
        }

        for component in self {
            f.write_char('/')?;
            f.write_str(component)?;
        }

        Ok(())
    }
}

impl PathVec {
    pub fn iter<'a>(&'a self) -> PathVecIter<'a> {
        self.components.iter().map(String::as_str)
    }

    pub fn as_path_buf(&self, prefix: &std::path::Path) -> std::path::PathBuf {
        let suffix = std::path::PathBuf::from_iter(self.components.iter());
        prefix.join(suffix)
    }

    pub fn is_root(&self) -> bool {
        self.components.is_empty()
    }

    pub fn from_base(mut prefix: Self, suffix: &str) -> Self {
        prefix.components.push(suffix.to_owned());
        prefix
    }

    pub fn base(&self) -> Option<PathVec> {
        if self.is_root() {
            None
        } else {
            Some(PathVec {
                components: self.components[0..self.components.len() - 1].to_vec(),
            })
        }
    }

    pub fn suffix(&self, prefix: &Self) -> Self {
        let mut components = Vec::new();
        let mut i1 = self.iter();
        let mut i2 = prefix.iter();

        loop {
            let c1 = i1.next();
            let c2 = i2.next();

            if let Some(component) = c1 {
                if let Some(component2) = c2 {
                    assert_eq!(component, component2);
                } else {
                    components.push(component.to_owned());
                }
            } else {
                return Self { components };
            }
        }
    }

    pub fn as_path_ref(&self) -> PathRef<'_> {
        self.into()
    }
}

#[cfg(test)]
mod test {
    use super::PathVec;
    use alloc::borrow::ToOwned;

    #[test]
    fn test_pathvec_from_iter() {
        let path = &["hello", "world"];
        let path = PathVec::from_iter(path.iter().copied());
        let components: alloc::vec::Vec<alloc::string::String> =
            path.iter().map(|x| x.to_owned()).collect();

        assert_eq!(components, &["hello", "world",]);
    }

    #[test]
    fn test_pathvec_into_iterator() {
        let path = &["hello", "world"];
        let path = PathVec::from_iter(path.iter().copied());
        let components: alloc::vec::Vec<alloc::string::String> =
            path.into_iter().map(|x| x.to_owned()).collect();

        assert_eq!(components, &["hello", "world",]);
    }

    #[test]
    fn test_pathvec_from_str() {
        let path = "/hello/world";
        let path = PathVec::from(path);
        let components: alloc::vec::Vec<alloc::string::String> =
            path.iter().map(|x| x.to_owned()).collect();

        assert_eq!(components, &["hello", "world",]);
    }

    #[test]
    fn test_pathvec_is_root_root_path() {
        let path = PathVec::default();
        assert!(path.is_root());
    }

    #[test]
    fn test_pathvec_is_root_non_root_path() {
        let path = PathVec::default();
        let path = PathVec::from_base(path, "nonroot");
        assert!(!path.is_root());
    }

    #[test]
    fn test_pathvec_base_root() {
        assert_eq!(PathVec::default().base(), None);
    }

    #[test]
    fn test_pathvec_base_non_root() {
        let path = PathVec::default();
        let path = PathVec::from_base(path, "nonroot");

        assert_eq!(path.base(), Some(PathVec::default()));
    }

    #[test]
    fn test_pathvec_to_path_buf() {
        let path = PathVec::default();
        let path = PathVec::from_base(path, "nonroot");
        let base = std::path::PathBuf::from("test");

        let path = path.as_path_buf(&base);
        assert_eq!(path, base.join("nonroot"));
    }

    #[test]
    fn test_pathvec_suffix() {
        let root = PathVec::default();
        let prefix = PathVec::from_base(root, "foo");
        let path = PathVec::from_base(prefix.clone(), "bar");
        let path = PathVec::from_base(path, "baz");

        let suffix = path.suffix(&prefix);
        assert_eq!(suffix, PathVec::from_iter(["bar", "baz",].into_iter()));
    }

    #[test]
    fn test_pathvec_to_string() {
        use alloc::string::ToString;

        let root = PathVec::default();
        let path = PathVec::from_base(root, "hello");
        let path = PathVec::from_base(path, "world");

        assert_eq!(path.to_string(), "/hello/world");
    }

    #[test]
    fn test_pathvec_to_string_root() {
        use alloc::string::ToString;

        let root = PathVec::default();
        assert_eq!(root.to_string(), "/");
    }
}
