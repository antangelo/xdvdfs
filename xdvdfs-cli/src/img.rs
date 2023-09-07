use ciso::read::CSOReader;
use maybe_async::maybe_async;
use std::{fs::File, io::BufReader, path::Path};
use xdvdfs::blockdev::{BlockDeviceRead, OffsetWrapper};

pub struct CSOBlockDevice<R: ciso::read::Read<std::io::Error>> {
    inner: CSOReader<std::io::Error, R>,
}

#[maybe_async(?Send)]
impl<R> BlockDeviceRead<std::io::Error> for CSOBlockDevice<R>
where
    R: ciso::read::Read<std::io::Error>,
{
    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), std::io::Error> {
        self.inner
            .read_offset(offset, buffer)
            .await
            .map_err(|e| match e {
                ciso::layout::Error::Other(e) => e,
                e => std::io::Error::new(std::io::ErrorKind::Other, e),
            })
    }
}

#[maybe_async(?Send)]
pub async fn open_image_raw(
    path: &Path,
) -> Result<OffsetWrapper<BufReader<File>, std::io::Error>, anyhow::Error> {
    let img = File::options().read(true).open(path)?;
    let img = std::io::BufReader::new(img);
    Ok(xdvdfs::blockdev::OffsetWrapper::new(img).await?)
}

#[maybe_async(?Send)]
pub async fn open_image(
    path: &Path,
) -> Result<Box<dyn BlockDeviceRead<std::io::Error>>, anyhow::Error> {
    if path.extension().is_some_and(|e| e == "cso") {
        let file_base = path.with_extension("");
        let split = file_base.extension().is_some_and(|e| e == "1");

        let reader: Box<dyn ciso::read::Read<std::io::Error>> = if split {
            let mut files = Vec::new();
            for i in 1.. {
                let part = file_base.with_extension(format!("{}.cso", i));
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
