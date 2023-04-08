#![no_std]

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "alloc")]
extern crate alloc;

const VERBOSE: bool = false;
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

#[cfg(feature = "read")]
pub mod read;
