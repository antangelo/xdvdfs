use alloc::boxed::Box;
use core::{
    iter::{Chain, Filter},
    str::Split,
};

use crate::write::fs::PathVec;

/// Opaque reference to a path
/// Internally implemented as an enum of path implementation variants
#[derive(Debug, Clone, Copy, Eq, Hash)]
pub enum PathRef<'a> {
    // String with '/' separator
    Str(&'a str),

    // Slice of components
    Slice(&'a [&'a str]),

    // PathVec reference
    PathVec(&'a PathVec),

    // PathRef joined with a string component
    Join(&'a PathRef<'a>, &'a str),
}

fn empty_component_filter(a: &&str) -> bool {
    !a.is_empty()
}

type StrIterType<'a> = Filter<Split<'a, &'static str>, for<'b> fn(&'b &'a str) -> bool>;

pub enum BasePathRefIter<'a> {
    Str(StrIterType<'a>),
    Slice(core::slice::Iter<'a, &'a str>),
    PathVec(super::PathVecIter<'a>),
}

pub enum PathRefIter<'a> {
    Base(BasePathRefIter<'a>),

    // Split out single joins to avoid an allocation
    // If we join more than once (e.g. "/".into().join("a").join("b"))
    // then box the iterators.
    SingleJoin(Chain<BasePathRefIter<'a>, core::iter::Once<&'a str>>),
    BoxedJoin(Chain<Box<PathRefIter<'a>>, core::iter::Once<&'a str>>),
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
            PathRef::Join(base, tail) => PathVec::from_base(&PathVec::from(*base), tail),
        }
    }
}

impl<'a> PathRef<'a> {
    pub fn iter(&self) -> PathRefIter<'a> {
        match self {
            Self::Str(s) => PathRefIter::Base(BasePathRefIter::Str(
                s.split("/").filter(empty_component_filter),
            )),
            Self::Slice(s) => PathRefIter::Base(BasePathRefIter::Slice(s.iter())),
            Self::PathVec(pv) => PathRefIter::Base(BasePathRefIter::PathVec(pv.iter())),
            Self::Join(base, tail) => {
                let tail_iter = core::iter::once(*tail);
                match base.iter() {
                    PathRefIter::Base(base_iter) => {
                        PathRefIter::SingleJoin(base_iter.chain(tail_iter))
                    }
                    base_iter => PathRefIter::BoxedJoin(Box::from(base_iter).chain(tail_iter)),
                }
            }
        }
    }

    pub fn join(&'a self, name: &'a str) -> Self {
        Self::Join(self, name)
    }
}

impl<'a> Iterator for BasePathRefIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Str(iter) => iter.next(),
            Self::Slice(iter) => iter.next().map(|s| &**s),
            Self::PathVec(iter) => iter.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Str(iter) => iter.size_hint(),
            Self::Slice(iter) => iter.size_hint(),
            Self::PathVec(iter) => iter.size_hint(),
        }
    }
}

impl<'a> Iterator for PathRefIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Base(iter) => iter.next(),
            Self::BoxedJoin(iter) => iter.next(),
            Self::SingleJoin(iter) => iter.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Base(iter) => iter.size_hint(),
            Self::BoxedJoin(iter) => iter.size_hint(),
            Self::SingleJoin(iter) => iter.size_hint(),
        }
    }
}

impl<'a, 'b: 'a> PartialEq<PathRef<'b>> for PathRef<'a> {
    fn eq(&self, other: &PathRef<'b>) -> bool {
        // Avoid potential allocation if both paths are joined
        if let Self::Join(base, tail) = self {
            if let Self::Join(other_base, other_tail) = other {
                return tail == other_tail && base == other_base;
            }
        }

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

impl PartialEq<PathVec> for PathRef<'_> {
    fn eq(&self, other: &PathVec) -> bool {
        self.eq(&PathRef::from(other))
    }
}

impl PartialEq<&str> for PathRef<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.eq(&PathRef::from(*other))
    }
}

impl PartialEq<&[&'_ str]> for PathRef<'_> {
    fn eq(&self, other: &&[&str]) -> bool {
        self.eq(&PathRef::from(*other))
    }
}

impl Ord for PathRef<'_> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        use core::cmp::Ordering;

        // Handle joined paths recursively to avoid allocating
        // in the iterator for each comparison
        if let (Self::Join(p1_rest, p1_tail), Self::Join(p2_rest, p2_tail)) = (self, other) {
            return match p1_rest.cmp(p2_rest) {
                Ordering::Equal => p1_tail.cmp(p2_tail),
                cmp => cmp,
            };
        }

        let mut p1_iter = self.iter();
        let mut p2_iter = other.iter();
        loop {
            match (p1_iter.next(), p2_iter.next()) {
                (Some(component_1), Some(component_2)) => match component_1.cmp(component_2) {
                    Ordering::Equal => continue,
                    cmp => break cmp,
                },
                (None, None) => break Ordering::Equal,
                (None, _) => break Ordering::Less,
                (_, None) => break Ordering::Greater,
            }
        }
    }
}

impl<'a, 'b: 'a> PartialOrd<PathRef<'b>> for PathRef<'a> {
    fn partial_cmp(&self, other: &PathRef<'a>) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
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
    fn test_pathref_to_pathvec_join() {
        use alloc::borrow::ToOwned;

        let path = "/hello/world";
        let path: PathRef = path.into();
        let path = path.join("abc");
        let path: super::PathVec = path.into();
        let components: alloc::vec::Vec<alloc::string::String> =
            path.iter().map(|x| x.to_owned()).collect();

        assert_eq!(components, &["hello", "world", "abc"]);
    }

    #[test]
    fn test_pathref_iter_join_single() {
        use alloc::borrow::ToOwned;

        let path = "/hello";
        let path: PathRef = path.into();
        let path = path.join("world");
        let components: alloc::vec::Vec<alloc::string::String> =
            path.iter().map(|x| x.to_owned()).collect();

        assert_eq!(components, &["hello", "world"]);
    }

    #[test]
    fn test_pathref_iter_join_depth() {
        use alloc::borrow::ToOwned;

        let path = "/hello";
        let path: PathRef = path.into();
        let path = path.join("world");
        let path = path.join("abc");
        let components: alloc::vec::Vec<alloc::string::String> =
            path.iter().map(|x| x.to_owned()).collect();

        assert_eq!(components, &["hello", "world", "abc"]);
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

    #[test]
    fn test_pathref_eq_both_joined() {
        let hello: PathRef = "/hello/".into();
        let p1 = hello.join("world");
        let p2 = hello.join("world");

        assert_eq!(p1, p2);
    }

    #[test]
    fn test_pathref_eq_one_joined() {
        let hello: PathRef = "/hello/".into();
        let p1 = hello.join("world");
        let p2: PathRef = "/hello/world".into();

        assert_eq!(p1, p2);
        assert_eq!(p2, p1);
    }

    #[test]
    fn test_pathref_eq_both_joined_depth() {
        let hello: PathRef = "/hello/".into();
        let p1 = hello.join("world");
        let p1 = p1.join("abc");
        let p2 = hello.join("world");
        let p2 = p2.join("abc");

        assert_eq!(p1, p2);
    }

    #[test]
    fn test_pathref_str_eq() {
        let hello: PathRef = "/hello/world".into();

        assert_eq!(hello, "/hello/world");
    }

    #[test]
    fn test_pathref_slice_eq() {
        let hello: PathRef = "/hello/world".into();
        assert_eq!(hello, ["hello", "world"].as_slice());
    }

    #[test]
    fn test_pathref_pathvec_eq() {
        let hello: PathRef = "/hello/world".into();
        let hello_pv: super::PathVec = "/hello/world".into();
        assert_eq!(hello, hello_pv);
    }

    #[test]
    fn test_pathref_cmp_non_joined_equal() {
        let path1: PathRef = "/hello/world".into();
        let path2: PathRef = "/hello/world".into();

        assert_eq!(path1.cmp(&path2), core::cmp::Ordering::Equal);
    }

    #[test]
    fn test_pathref_cmp_non_joined_lt() {
        let path1: PathRef = "/hello/world".into();
        let path2: PathRef = "/hello/worlds".into();

        assert!(path1 < path2);
    }

    #[test]
    fn test_pathref_cmp_non_joined_lt_component() {
        let path1: PathRef = "/hello".into();
        let path2: PathRef = "/hello/world".into();

        assert!(path1 < path2);
    }

    #[test]
    fn test_pathref_cmp_non_joined_gt_component() {
        let path1: PathRef = "/hello/world".into();
        let path2: PathRef = "/hello".into();

        assert!(path1 > path2);
    }

    #[test]
    fn test_pathref_cmp_joined_eq() {
        let hello = PathRef::from("hello");
        let path1 = PathRef::Join(&hello, "world");
        let path2 = PathRef::Join(&hello, "world");

        assert_eq!(path1.cmp(&path2), core::cmp::Ordering::Equal);
    }

    #[test]
    fn test_pathref_cmp_joined_lt_tail() {
        let hello = PathRef::from("hello");
        let path1 = PathRef::Join(&hello, "world");
        let path2 = PathRef::Join(&hello, "worlds");

        assert!(path1 < path2);
    }

    #[test]
    fn test_pathref_cmp_joined_gt_base() {
        let hello = PathRef::from("hello");
        let path1 = PathRef::Join(&hello, "world");
        let greetings = PathRef::from("greetings");
        let path2 = PathRef::Join(&greetings, "world");

        assert!(path1 > path2);
    }
}
