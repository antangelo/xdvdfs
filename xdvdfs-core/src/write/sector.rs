use crate::layout::SECTOR_SIZE;

pub struct SectorAllocator {
    next_free: usize,
}

impl Default for SectorAllocator {
    fn default() -> Self {
        // FIXME: Allow files to fill sectors 0..=31
        Self { next_free: 33 }
    }
}

impl SectorAllocator {
    /// Allocates a contiguous set of sectors, big enough to fit `bytes`.
    /// Returns the number of the first sector in the allocation
    pub fn allocate_contiguous(&mut self, bytes: usize) -> usize {
        let sectors = bytes / SECTOR_SIZE + if bytes % SECTOR_SIZE > 0 { 1 } else { 0 };
        let allocation = self.next_free;
        self.next_free += sectors;
        allocation
    }
}
