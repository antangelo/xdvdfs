use crate::layout::SECTOR_SIZE;

pub struct SectorAllocator {
    next_free: u64,
}

impl Default for SectorAllocator {
    fn default() -> Self {
        // FIXME: Allow files to fill sectors 0..=31
        Self { next_free: 33 }
    }
}

pub fn required_sectors(len: u64) -> u64 {
    if len != 0 {
        len / SECTOR_SIZE + if len % SECTOR_SIZE > 0 { 1 } else { 0 }
    } else {
        1
    }
}

impl SectorAllocator {
    /// Allocates a contiguous set of sectors, big enough to fit `bytes`.
    /// Returns the number of the first sector in the allocation
    pub fn allocate_contiguous(&mut self, bytes: u64) -> u64 {
        let sectors = required_sectors(bytes);
        let allocation = self.next_free;
        self.next_free += sectors;
        allocation
    }
}
