use alloc::boxed::Box;

pub struct PathRef<'a>(Box<dyn Iterator<Item = &'a str> + 'a>);

impl PathRef<'_> {
    pub fn new<'a, I: Iterator<Item = &'a str> + 'a>(iter: I) -> PathRef<'a> {
        PathRef(Box::new(iter))
    }
}

impl<'a> From<&'a str> for PathRef<'a> {
    fn from(value: &'a str) -> Self {
        PathRef::new(value.split("/").filter(|component| !component.is_empty()))
    }
}

impl<'a> Iterator for PathRef<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn test_str_to_pathref_components() {
        use alloc::borrow::ToOwned;

        let path = "/hello/world/";
        let path: super::PathRef<'_> = path.into();
        let components: alloc::vec::Vec<alloc::string::String> =
            path.map(|component| component.to_owned()).collect();

        assert_eq!(components, &["hello", "world",]);
    }
}
