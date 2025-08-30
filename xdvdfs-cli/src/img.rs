use ciso::read::CSOReader;
use maybe_async::maybe_async;
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};
use xdvdfs::blockdev::{BlockDeviceRead, OffsetWrapper};

pub struct CSOBlockDevice<R: ciso::read::Read<ReadError = std::io::Error>> {
    inner: CSOReader<std::io::Error, R>,
}

#[maybe_async]
impl<R> BlockDeviceRead for CSOBlockDevice<R>
where
    R: ciso::read::Read<ReadError = std::io::Error>,
{
    type ReadError = std::io::Error;

    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), std::io::Error> {
        self.inner
            .read_offset(offset, buffer)
            .await
            .map_err(|e| match e {
                ciso::layout::Error::Other(e) => e,
                e => std::io::Error::other(e),
            })
    }

    async fn image_size(&mut self) -> Result<u64, std::io::Error> {
        Ok(self.inner.file_size())
    }
}

#[maybe_async]
pub async fn open_image_raw(path: &Path) -> Result<OffsetWrapper<BufReader<File>>, anyhow::Error> {
    let img = File::options().read(true).open(path)?;
    let img = std::io::BufReader::new(img);
    Ok(xdvdfs::blockdev::OffsetWrapper::new(img).await?)
}

#[maybe_async]
pub async fn open_image(
    path: &Path,
) -> Result<Box<dyn BlockDeviceRead<ReadError = std::io::Error>>, anyhow::Error> {
    if path.extension().is_some_and(|e| e == "cso") {
        let file_base = path.with_extension("");
        let split = file_base.extension().is_some_and(|e| e == "1");

        let reader: Box<dyn ciso::read::Read<ReadError = std::io::Error>> = if split {
            let mut files = Vec::new();
            for i in 1.. {
                let part = file_base.with_extension(format!("{i}.cso"));
                if !part.exists() {
                    break;
                }

                let part = std::io::BufReader::new(std::fs::File::open(part)?);
                files.push(part);
            }

            if files.is_empty() {
                return Err(anyhow::anyhow!("Failed to open file {:?}", path));
            }

            Box::from(ciso::split::SplitFileReader::new(files).await?)
        } else {
            let file = std::io::BufReader::new(std::fs::File::open(path)?);
            Box::from(file)
        };

        let reader = ciso::read::CSOReader::new(reader).await?;
        let reader = Box::from(CSOBlockDevice { inner: reader });
        Ok(reader)
    } else {
        let image = open_image_raw(path).await?;
        let image = Box::from(image);
        Ok(image)
    }
}

/// Returns the result of std::fs::canonicalize, if it returned without error,
/// otherwise returns the result of std::path::absolute
/// std::fs::canonicalize will follow symlinks, but seems to error on some filesystems
/// on Windows. Following symlinks is desirable, so it is attempted first
pub fn absolute_path(path: &Path) -> std::io::Result<PathBuf> {
    match std::fs::canonicalize(path) {
        Err(_) => std::path::absolute(path),
        ok => ok,
    }
}

/// Similar to Path::with_extension, but will not overwrite the extension for
/// directories
// TODO: Replace with `Path::with_added_extension` after it stabilizes
pub fn with_extension(path: &Path, ext: &str, is_dir: bool) -> PathBuf {
    if !is_dir {
        return path.with_extension(ext);
    }

    let original_ext = path.extension();
    let Some(original_ext) = original_ext else {
        return path.with_extension(ext);
    };

    let mut new_ext = original_ext.to_owned();
    new_ext.push(".");
    new_ext.push(ext);
    path.with_extension(new_ext)
}

#[cfg(test)]
mod test {
    use super::with_extension;
    use std::path::Path;

    #[test]
    fn with_extension_not_dir() {
        assert_eq!(
            with_extension(Path::new("file.abc"), "xyz", false),
            Path::new("file.xyz")
        );
    }

    #[test]
    fn with_extension_dir_no_extension() {
        assert_eq!(
            with_extension(Path::new("dir"), "xyz", true),
            Path::new("dir.xyz")
        );
    }

    #[test]
    fn with_extension_dir_with_extension() {
        assert_eq!(
            with_extension(Path::new("dir.abc"), "xyz", true),
            Path::new("dir.abc.xyz")
        );
    }
}
