use crate::layout::SECTOR_SIZE;

pub struct SectorAllocator {
    next_free: u32,
}

impl Default for SectorAllocator {
    fn default() -> Self {
        // FIXME: Allow files to fill sectors 0..=31
        Self { next_free: 33 }
    }
}

pub fn required_sectors(len: u64) -> u32 {
    if len != 0 {
        let sectors: u32 = (len / SECTOR_SIZE as u64).try_into().expect("number of sectors should fit in u32");
        sectors + if (len % SECTOR_SIZE as u64) as u32 > 0 { 1 } else { 0 }
    } else {
        1
    }
}

impl SectorAllocator {
    /// Allocates a contiguous set of sectors, big enough to fit `bytes`.
    /// Returns the number of the first sector in the allocation
    pub fn allocate_contiguous(&mut self, bytes: u64) -> u32 {
        let sectors = required_sectors(bytes);
        let allocation = self.next_free;
        self.next_free += sectors;
        allocation
    }
}
