use std::{
    borrow::Cow,
    io::{StdoutLock, Write},
    path::Path,
};

use xdvdfs::write::img::ProgressVisitor;

pub struct StdIoProgressReporter<'source> {
    stdio: StdoutLock<'static>,
    source_prefix: Cow<'source, str>,
    file_count: usize,
    progress_count: usize,
}

impl<'source> StdIoProgressReporter<'source> {
    pub fn new(source_path: &'source Path, is_dir: bool) -> Self {
        let source_prefix = if is_dir {
            source_path.to_string_lossy()
        } else {
            Cow::Borrowed("")
        };

        Self {
            stdio: std::io::stdout().lock(),
            source_prefix,
            file_count: 0,
            progress_count: 0,
        }
    }
}

impl<'source> ProgressVisitor for StdIoProgressReporter<'source> {
    fn entry_counts(&mut self, file_count: usize, dir_count: usize) {
        self.file_count += file_count;
        self.file_count += dir_count;
    }

    fn directory_added(&mut self, path: xdvdfs::write::fs::PathRef<'_>, sector: u64) {
        self.progress_count += 1;
        let _ = writeln!(
            self.stdio,
            "[{}/{}] Added dir: {}{path} at sector {sector}",
            self.progress_count, self.file_count, self.source_prefix,
        );
    }

    fn file_added(&mut self, path: xdvdfs::write::fs::PathRef<'_>, sector: u64) {
        self.progress_count += 1;
        let _ = writeln!(
            self.stdio,
            "[{}/{}] Added file: {}{path} at sector {sector}",
            self.progress_count, self.file_count, self.source_prefix,
        );
    }
}
