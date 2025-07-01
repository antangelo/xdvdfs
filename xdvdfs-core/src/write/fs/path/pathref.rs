use core::{iter::Filter, str::Split};

use crate::write::fs::PathVec;

/// Opaque reference to a path
/// Internally implemented as an enum of path implementation variants
#[derive(Debug)]
pub enum PathRef<'a> {
    // String with '/' separator
    Str(&'a str),

    // Slice of components
    Slice(&'a [&'a str]),

    // PathVec reference
    PathVec(&'a PathVec),
}

fn empty_component_filter(a: &&str) -> bool {
    !a.is_empty()
}

type StrIterType<'a> = Filter<Split<'a, &'static str>, for<'b> fn(&'b &'a str) -> bool>;

pub enum PathRefIter<'a> {
    Str(StrIterType<'a>),
    Slice(core::slice::Iter<'a, &'a str>),
    PathVec(super::PathVecIter<'a>),
}

impl<'a> From<&'a str> for PathRef<'a> {
    fn from(value: &'a str) -> Self {
        PathRef::Str(value)
    }
}

impl<'a> From<&'a [&'a str]> for PathRef<'a> {
    fn from(value: &'a [&'a str]) -> Self {
        PathRef::Slice(value)
    }
}

impl<'a> From<&'a PathVec> for PathRef<'a> {
    fn from(value: &'a PathVec) -> Self {
        PathRef::PathVec(value)
    }
}

impl<'a> From<PathRef<'a>> for PathVec {
    fn from(value: PathRef<'a>) -> Self {
        match value {
            PathRef::Str(s) => PathVec::from(s),
            PathRef::Slice(sl) => PathVec::from_iter(sl.iter().map(|s| &**s)),
            PathRef::PathVec(pv) => pv.clone(),
        }
    }
}

impl<'a> PathRef<'a> {
    pub fn iter(&self) -> PathRefIter<'a> {
        match self {
            Self::Str(s) => PathRefIter::Str(s.split("/").filter(empty_component_filter)),
            Self::Slice(s) => PathRefIter::Slice(s.iter()),
            Self::PathVec(pv) => PathRefIter::PathVec(pv.iter()),
        }
    }
}

impl<'a> Iterator for PathRefIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            PathRefIter::Str(iter) => iter.next(),
            PathRefIter::Slice(iter) => iter.next().map(|s| &**s),
            PathRefIter::PathVec(iter) => iter.next(),
        }
    }
}

impl<'a, 'b> PartialEq<PathRef<'b>> for PathRef<'a> {
    fn eq(&self, other: &PathRef<'b>) -> bool {
        let mut i1 = self.iter();
        let mut i2 = other.iter();

        loop {
            match (i1.next(), i2.next()) {
                (Some(c1), Some(c2)) if c1 == c2 => continue,
                (None, None) => break true,
                _ => break false,
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::PathRef;

    #[test]
    fn test_str_to_pathref() {
        use alloc::borrow::ToOwned;

        let path = "/hello/world/";
        let path: PathRef = path.into();
        let components: alloc::vec::Vec<alloc::string::String> =
            path.iter().map(|component| component.to_owned()).collect();

        assert_eq!(components, &["hello", "world",]);
    }

    #[test]
    fn test_slice_to_pathref() {
        use alloc::borrow::ToOwned;

        let path = &["hello", "world"].as_slice();
        let path: PathRef = (*path).into();
        let components: alloc::vec::Vec<alloc::string::String> =
            path.iter().map(|component| component.to_owned()).collect();

        assert_eq!(components, &["hello", "world",]);
    }

    #[test]
    fn test_pathvec_to_pathref() {
        use super::PathVec;
        use alloc::borrow::ToOwned;

        let path = PathVec::from_base(&PathVec::default(), "hello");
        let path = PathVec::from_base(&path, "world");
        let path: PathRef = (&path).into();
        let components: alloc::vec::Vec<alloc::string::String> =
            path.iter().map(|x| x.to_owned()).collect();

        assert_eq!(components, &["hello", "world",]);
    }

    #[test]
    fn test_pathref_to_pathvec_string() {
        use alloc::borrow::ToOwned;

        let path = "/hello/world";
        let path: PathRef = path.into();
        let path: super::PathVec = path.into();
        let components: alloc::vec::Vec<alloc::string::String> =
            path.iter().map(|x| x.to_owned()).collect();

        assert_eq!(components, &["hello", "world",]);
    }

    #[test]
    fn test_pathref_to_pathvec_slice() {
        use alloc::borrow::ToOwned;

        let path = &["hello", "world"].as_slice();
        let path: PathRef = (*path).into();
        let path: super::PathVec = path.into();
        let components: alloc::vec::Vec<alloc::string::String> =
            path.iter().map(|x| x.to_owned()).collect();

        assert_eq!(components, &["hello", "world",]);
    }

    #[test]
    fn test_pathref_to_pathvec_pathvec() {
        use alloc::borrow::ToOwned;

        let path = super::PathVec::from("/hello/world");
        let path: PathRef = (&path).into();
        let path: super::PathVec = path.into();
        let components: alloc::vec::Vec<alloc::string::String> =
            path.iter().map(|x| x.to_owned()).collect();

        assert_eq!(components, &["hello", "world",]);
    }

    #[test]
    fn test_pathref_eq_equal() {
        let p1: PathRef = "/hello/world".into();
        let p2: PathRef = "/hello/world".into();

        assert_eq!(p1, p2);
    }

    #[test]
    fn test_pathref_eq_unequal_components() {
        let p1: PathRef = "/hello/world".into();
        let p2: PathRef = "/hello/universe".into();

        assert_ne!(p1, p2);
    }

    #[test]
    fn test_pathref_eq_unequal_lengths() {
        let p1: PathRef = "/hello/world".into();
        let p2: PathRef = "/hello".into();

        assert_ne!(p1, p2);
    }
}
