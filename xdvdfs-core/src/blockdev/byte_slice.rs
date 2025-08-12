use core::fmt::Display;

use alloc::boxed::Box;
use maybe_async::maybe_async;

use core::error::Error;

use super::{BlockDeviceRead, BlockDeviceWrite};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct OutOfBounds;

impl Display for OutOfBounds {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("out of bounds")
    }
}

impl Error for OutOfBounds {}

#[maybe_async]
impl BlockDeviceRead for [u8] {
    type ReadError = OutOfBounds;

    async fn read(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), OutOfBounds> {
        let offset = offset as usize;
        let source_len = self.as_ref().len();
        let size = <[u8]>::len(buffer);
        if offset >= source_len || source_len - offset < size {
            return Err(OutOfBounds);
        }

        let range = offset..(offset + size);
        buffer.copy_from_slice(&self[range]);
        Ok(())
    }
}

#[maybe_async]
impl BlockDeviceWrite for [u8] {
    type WriteError = OutOfBounds;

    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), Self::WriteError> {
        let offset: usize = offset.try_into().map_err(|_| OutOfBounds)?;
        let buffer_size = <[u8]>::len(self);
        if offset >= buffer_size || buffer_size - offset < buffer.len() {
            return Err(OutOfBounds);
        }

        self[offset..(offset + buffer.len())].copy_from_slice(buffer);
        Ok(())
    }

    async fn len(&mut self) -> Result<u64, Self::WriteError> {
        Ok(<[u8]>::len(self) as u64)
    }
}

// TODO: Remove this impl, it is covered by [u8] via as_mut_slice()
#[maybe_async]
impl BlockDeviceWrite for Box<[u8]> {
    type WriteError = OutOfBounds;

    async fn write(&mut self, offset: u64, buffer: &[u8]) -> Result<(), Self::WriteError> {
        let offset: usize = offset.try_into().map_err(|_| OutOfBounds)?;
        let buffer_size = <[u8]>::len(self);
        if offset >= buffer_size || buffer_size - offset < buffer.len() {
            return Err(OutOfBounds);
        }

        self[offset..(offset + buffer.len())].copy_from_slice(buffer);
        Ok(())
    }

    async fn len(&mut self) -> Result<u64, Self::WriteError> {
        Ok(<[u8]>::len(self) as u64)
    }
}

#[cfg(test)]
mod test {
    use futures::executor::block_on;

    use crate::blockdev::{BlockDeviceRead, BlockDeviceWrite, OutOfBounds};

    #[test]
    fn test_blockdev_byte_slice_read_offset_out_of_range() {
        let mut bytes = [1, 2, 3, 4, 5];
        let mut buffer = [0u8; 100];

        let res = block_on(bytes.as_mut_slice().read(6, &mut buffer));
        assert_eq!(res, Err(OutOfBounds));
    }

    #[test]
    fn test_blockdev_byte_slice_read_size_out_of_range() {
        let mut bytes = [1, 2, 3, 4, 5];
        let mut buffer = [0u8; 100];

        let res = block_on(bytes.as_mut_slice().read(0, &mut buffer));
        assert_eq!(res, Err(OutOfBounds));
    }

    #[test]
    fn test_blockdev_byte_slice_read_in_bounds() {
        let mut bytes = [1, 2, 3, 4, 5];
        let mut buffer = [0u8; 100];

        let res = block_on(bytes.as_mut_slice().read(1, &mut buffer[..3]));
        assert_eq!(res, Ok(()));
        assert_eq!(buffer[..3], [2, 3, 4]);
    }

    #[test]
    fn test_blockdev_byte_slice_write_offset_out_of_range() {
        let mut bytes = [1, 2, 3, 4, 5];
        let mut buffer = [0u8; 100];

        let res = block_on(buffer.as_mut_slice().write(105, &mut bytes));
        assert_eq!(res, Err(OutOfBounds));
    }

    #[test]
    fn test_blockdev_byte_slice_write_size_out_of_range() {
        let mut bytes = [1, 2, 3, 4, 5];
        let mut buffer = [0u8; 100];

        let res = block_on(buffer.as_mut_slice().write(98, &mut bytes));
        assert_eq!(res, Err(OutOfBounds));
    }

    #[test]
    fn test_blockdev_byte_slice_write_in_bounds() {
        let mut bytes = [1, 2, 3, 4, 5];
        let mut buffer = [0u8; 100];

        let res = block_on(buffer.as_mut_slice().write(50, &mut bytes));
        assert_eq!(res, Ok(()));
        assert_eq!(buffer[50..55], [1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_blockdev_byte_slice_write_len() {
        let mut bytes = [1, 2, 3, 4, 5];
        let mut buffer = [0u8; 100];

        let res = block_on(buffer.as_mut_slice().write(50, &mut bytes));
        assert_eq!(res, Ok(()));
        assert_eq!(buffer[50..55], [1, 2, 3, 4, 5]);

        let len =
            block_on(buffer.as_mut_slice().len()).expect("Len should be computed without error");
        assert_eq!(len, 100);
    }

    #[test]
    fn test_blockdev_byte_slice_write_boxed_in_bounds() {
        let mut bytes = [1, 2, 3, 4, 5];
        let mut buffer = alloc::boxed::Box::new([0u8; 100]);

        let res = block_on(buffer.write(50, &mut bytes));
        assert_eq!(res, Ok(()));
        assert_eq!(buffer[50..55], [1, 2, 3, 4, 5]);

        let len = block_on(BlockDeviceWrite::len(buffer.as_mut_slice()))
            .expect("Len should be computed without error");
        assert_eq!(len, 100);
    }
}
