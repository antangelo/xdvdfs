#![no_std]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

#[allow(unused)]
const VERBOSE: bool = false;

#[allow(unused)]
macro_rules! dprintln {
    ($($x:expr),*) => {
        #[cfg(feature = "std")]
        if crate::VERBOSE {
            std::eprintln!($($x),*);
        }
    };
}

pub mod blockdev;
pub mod layout;
pub mod util;

#[cfg(feature = "read")]
pub mod read;

#[cfg(feature = "write")]
pub mod write;
