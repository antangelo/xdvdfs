#![no_std]
// TODO: Re-enable and fix lint once Nix stable build is fixed
// `is_multiple_of` is too new (Rust 1.87) and the lint is added
// in 1.89, neither of which are in Nix stable currently
#![allow(unknown_lints)]
#![allow(clippy::manual_is_multiple_of)]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

macro_rules! traceln {
    ($($x:expr),*) => {
        #[cfg(all(feature = "std", feature = "logging"))]
        log::trace!($($x),*);
    };
}

macro_rules! debugln {
    ($($x:expr),*) => {
        #[cfg(all(feature = "std", feature = "logging"))]
        log::debug!($($x),*);
    };
}

pub mod blockdev;
pub mod layout;
pub mod util;

#[cfg(feature = "read")]
pub mod read;

#[cfg(feature = "write")]
pub mod write;

#[cfg(all(feature = "checksum", feature = "std"))]
pub mod checksum;
