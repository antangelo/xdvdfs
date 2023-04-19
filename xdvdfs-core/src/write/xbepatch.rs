use proc_bitfield::bitfield;
// FIXME: Move all XBE stuff into its own crate
use bincode::Options;
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

use crate::{
    blockdev::{BlockDeviceRead, BlockDeviceWrite},
    layout::DiskRegion,
    util,
};

pub const XBE_HEADER_MAGIC: u32 = 0x48454258;

#[repr(C)]
#[repr(packed)]
#[derive(Serialize, Deserialize, Copy, Clone, Debug)]
pub struct XbeHeader {
    xbeh: u32,

    #[serde(with = "BigArray")]
    signature: [u8; 256],

    base_addr: u32,
    header_size: u32,

    image_size: u32,
    image_header_size: u32,

    timestamp: u32,

    cert_addr: u32,
    section_count: u32,
    section_header_addr: u32,

    init_flags: u32,
    entry_point: u32,
    tls_address: u32,
    stack_size: u32,

    pe_heap_reserve: u32,
    pe_heap_commit: u32,
    pe_base_addr: u32,
    pe_image_size: u32,
    pe_checksum: u32,
    pe_timestamp: u32,

    debug_path: u32,
    debug_filename: u32,
    debug_filename_utf16: u32,

    kernel_thunk_addr: u32,
    import_dir_address: u32,

    library_version_count: u32,
    library_version_addr: u32,
    kernel_lib_version_addr: u32,
    xapi_lib_version_addr: u32,

    logo_bitmap_addr: u32,
    logo_bitmap_size: u32,

    unknown1: u64,
    unknown2: u32,
}

#[repr(C)]
#[repr(packed)]
#[derive(Serialize, Deserialize, Copy, Clone, Debug)]
pub struct XbeCertificate {
    size: u32,
    timestamp: u32,

    title_id: u32,

    #[serde(with = "BigArray")]
    title_name_utf16: [u16; 40],

    alt_title_id: [u32; 16],

    allowed_media_types: AllowedMediaTypes,

    game_region: u32,
    game_ratings: u32,
    disk_number: u32,
    version: u32,

    lan_key: [u8; 16],
    sig_key: [u8; 16],

    #[serde(with = "BigArray")]
    alternate_sig_key: [u8; 256],

    unknown1: u32,
    unknown2: u32,

    runtime_security_flags: u32,
}

bitfield!(
#[repr(C)]
#[derive(Serialize, Deserialize, Copy, Clone)]
pub struct AllowedMediaTypes(pub u32): Debug {
    pub types: u32 @ ..,

    pub hard_disk: bool @ 0,
    pub dvd_x2: bool @ 1,
    pub dvd_cd: bool @ 2,
    pub cd: bool @ 3,
    pub dvd_5_ro: bool @ 4,
    pub dvd_9_ro: bool @ 5,
    pub dvd_5_rw: bool @ 6,
    pub dvd_9_rw: bool @ 7,
    pub dongle: bool @ 8,
    pub media_board: bool @ 9,

    pub nonsecure_hard_disk: bool @ 30,
    pub nonsecure_mode: bool @ 31,
}
);

impl XbeHeader {
    pub fn is_valid(&self) -> bool {
        self.xbeh == XBE_HEADER_MAGIC
    }

    pub fn deserialize<E>(
        buf: &[u8; core::mem::size_of::<XbeHeader>()],
    ) -> Result<Self, util::Error<E>> {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .deserialize(buf)
            .map_err(|e| util::Error::SerializationFailed(e))
    }
}

impl XbeCertificate {
    #[cfg(feature = "alloc")]
    pub fn serialize<E>(&self) -> Result<alloc::vec::Vec<u8>, util::Error<E>> {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .serialize(self)
            .map_err(|e| util::Error::SerializationFailed(e))
    }

    pub fn deserialize<E>(
        buf: &[u8; core::mem::size_of::<XbeCertificate>()],
    ) -> Result<Self, util::Error<E>> {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .deserialize(buf)
            .map_err(|e| util::Error::SerializationFailed(e))
    }
}

#[derive(Copy, Clone, Debug)]
pub struct MediaPatchInfo {
    pub original: AllowedMediaTypes,
    pub new: AllowedMediaTypes,
}

#[cfg(feature = "alloc")]
pub fn apply_media_patch<T, E: core::fmt::Debug>(
    img: &mut T,
    xbe: DiskRegion,
) -> Result<MediaPatchInfo, util::Error<E>>
where
    T: BlockDeviceRead<E> + BlockDeviceWrite<E>,
{
    use alloc::vec;

    let xbe_header = {
        let mut xbe_header = vec![0; core::mem::size_of::<XbeHeader>()];
        let offset = xbe.offset(0)?;
        img.read(offset, &mut xbe_header)?;

        XbeHeader::deserialize(&xbe_header.try_into().unwrap())?
    };

    if !xbe_header.is_valid() {
        // FIXME: Better error message
        return Err(util::Error::InvalidVolume);
    }

    let mut xbe_cert = {
        let mut xbe_cert = vec![0; core::mem::size_of::<XbeCertificate>()];
        let offset = xbe.offset(xbe_header.cert_addr - xbe_header.base_addr)?;
        img.read(offset, &mut xbe_cert)?;

        XbeCertificate::deserialize(&xbe_cert.try_into().unwrap())?
    };

    let orig_media_type = xbe_cert.allowed_media_types;
    let mut new_media_type = xbe_cert.allowed_media_types;

    new_media_type.set_hard_disk(true);
    new_media_type.set_dvd_x2(true);
    new_media_type.set_dvd_cd(true);
    new_media_type.set_cd(true);
    new_media_type.set_dvd_5_ro(true);
    new_media_type.set_dvd_9_ro(true);
    new_media_type.set_dvd_5_rw(true);
    new_media_type.set_dvd_9_rw(true);
    new_media_type.set_dongle(true);
    new_media_type.set_media_board(true);
    new_media_type.set_nonsecure_mode(true);
    new_media_type.set_nonsecure_hard_disk(true);

    xbe_cert.allowed_media_types = new_media_type;
    xbe_cert.runtime_security_flags &= !1;

    let write_size = core::cmp::min(
        xbe_cert.size as usize,
        core::mem::size_of::<XbeCertificate>(),
    );
    let xbe_cert = xbe_cert.serialize()?;
    let offset = xbe.offset(xbe_header.cert_addr - xbe_header.base_addr)?;

    img.write(offset, &xbe_cert[0..write_size]).unwrap();

    Ok(MediaPatchInfo {
        original: orig_media_type,
        new: new_media_type,
    })
}
