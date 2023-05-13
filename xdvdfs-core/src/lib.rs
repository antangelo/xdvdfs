#![no_std]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

macro_rules! dprintln {
    ($($x:expr),*) => {
        #[cfg(all(feature = "std", feature = "logging"))]
        log::trace!($($x),*);
    };
}

pub mod blockdev;
pub mod layout;
pub mod util;

#[cfg(feature = "read")]
pub mod read;

#[cfg(feature = "write")]
pub mod write;
