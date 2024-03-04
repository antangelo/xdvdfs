use std::format;
use std::path::{Path, PathBuf};


pub trait PathUnix: AsRef<Path> {
    fn join_xdvdfs<S: AsRef<str>>(&self, other: S) -> PathBuf {
        let self_path = self.as_ref().to_string_lossy();

        PathBuf::from(format!(
            "{}/{}",
            if self_path == "/" { "" } else { self_path.as_ref() },
            other.as_ref()
        ))
    }
}

impl PathUnix for Path {}
impl PathUnix for PathBuf {}
