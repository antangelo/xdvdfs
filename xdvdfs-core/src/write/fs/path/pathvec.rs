use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::slice::Iter;

use super::PathRef;

#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathVec {
    components: Vec<String>,
}

pub type PathVecIter<'a> = Iter<'a, String>;

impl<'a> From<PathRef<'a>> for PathVec {
    fn from(value: PathRef<'a>) -> Self {
        let components: Vec<String> = value.map(|component| component.to_owned()).collect();
        Self { components }
    }
}

impl<'a> From<&'a PathVec> for PathRef<'a> {
    fn from(value: &'a PathVec) -> Self {
        PathRef::new(value.components.iter().map(|x| x.as_ref()))
    }
}

impl PathVec {
    pub fn as_path_buf(&self, prefix: &std::path::Path) -> std::path::PathBuf {
        let suffix = std::path::PathBuf::from_iter(self.components.iter());
        prefix.join(suffix)
    }

    pub fn is_root(&self) -> bool {
        self.components.is_empty()
    }

    pub fn iter(&self) -> PathVecIter<'_> {
        self.components.iter()
    }

    pub fn from_base(prefix: &Self, suffix: &str) -> Self {
        let mut path = prefix.clone();
        path.components.push(suffix.to_owned());
        path
    }

    pub fn as_string(&self) -> String {
        format!("/{}", self.components.join("/"))
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
                    components.push(component.clone());
                }
            } else {
                return Self { components };
            }
        }
    }
}

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

#[cfg(test)]
mod test {
    use super::PathVec;

    #[test]
    fn test_pathvec_to_pathref() {
        use alloc::borrow::ToOwned;

        let path = PathVec::from_base(&PathVec::default(), "hello");
        let path = PathVec::from_base(&path, "world");
        let path: super::PathRef<'_> = (&path).into();
        let components: alloc::vec::Vec<alloc::string::String> =
            path.map(|component| component.to_owned()).collect();

        assert_eq!(components, &["hello", "world",]);
    }

    #[test]
    fn test_pathref_to_pathvec() {
        let path = "/hello/world";
        let path: super::PathRef<'_> = path.into();
        let path: PathVec = path.into();
        let components: alloc::vec::Vec<alloc::string::String> = path.iter().cloned().collect();

        assert_eq!(components, &["hello", "world",]);
    }

    #[test]
    fn test_pathvec_from_iter() {
        let path = &["hello", "world"];
        let path = PathVec::from_iter(path.iter().copied());
        let components: alloc::vec::Vec<alloc::string::String> = path.iter().cloned().collect();

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
        let path = PathVec::from_base(&path, "nonroot");
        assert!(!path.is_root());
    }

    #[test]
    fn test_pathvec_base_root() {
        assert_eq!(PathVec::default().base(), None);
    }

    #[test]
    fn test_pathvec_base_non_root() {
        let path = PathVec::default();
        let path = PathVec::from_base(&path, "nonroot");

        assert_eq!(path.base(), Some(PathVec::default()));
    }

    #[test]
    fn test_pathvec_to_path_buf() {
        let path = PathVec::default();
        let path = PathVec::from_base(&path, "nonroot");
        let base = std::path::PathBuf::from("test");

        let path = path.as_path_buf(&base);
        assert_eq!(path, base.join("nonroot"));
    }

    #[test]
    fn test_pathvec_suffix() {
        let root = PathVec::default();
        let prefix = PathVec::from_base(&root, "foo");
        let path = PathVec::from_base(&prefix, "bar");
        let path = PathVec::from_base(&path, "baz");

        let suffix = path.suffix(&prefix);
        assert_eq!(suffix, PathVec::from_iter(["bar", "baz",].into_iter()));
    }

    #[test]
    fn test_pathvec_to_string() {
        let root = PathVec::default();
        let path = PathVec::from_base(&root, "hello");
        let path = PathVec::from_base(&path, "world");

        assert_eq!(path.as_string(), "/hello/world");
    }
}
