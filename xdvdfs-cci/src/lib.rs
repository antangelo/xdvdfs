//! CCI (`.cci`) — Xbox compressed disc image format as implemented by
//! [Team-Resurgent XboxToolkit](https://github.com/Team-Resurgent/XboxToolkit/blob/main/XboxToolkit/CCIContainerReader.cs).
//!
//! Each slice is a standalone container: 32-byte LE header, sector blobs, then a
//! `sectors + 1` × `u32` index. Split sets use `basename.1.cci`, `basename.2.cci`, …

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use lz4_flex::block::{compress_into, decompress_into, get_maximum_output_size};
use thiserror::Error;

pub const SECTOR_SIZE: usize = 2048;
pub const HEADER_SIZE: u64 = 32;
pub const MAGIC: u32 = 0x4D49_4343; // "CCIM" LE

const INDEX_ALIGN: u32 = 2;
const BLOCK_SIZE: u32 = SECTOR_SIZE as u32;

#[derive(Debug, Error)]
pub enum CciError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid CCI header magic")]
    BadMagic,
    #[error("unsupported CCI header_size {0} (expected 32)")]
    BadHeaderSize(u32),
    #[error("unsupported CCI block_size {0} (expected 2048)")]
    BadBlockSize(u32),
    #[error("unsupported CCI version {0} (expected 1)")]
    BadVersion(u8),
    #[error("unsupported CCI index_alignment {0} (expected 2)")]
    BadIndexAlign(u8),
    #[error("invalid uncompressed_size / sector count")]
    BadSize,
    #[error("CCI index read failed or truncated")]
    BadIndex,
    #[error("sector {0}: invalid compressed blob")]
    BadSector(u64),
    #[error("LZ4 decompress error: {0}")]
    Lz4(String),
    #[error("no CCI slices found for {0:?}")]
    NoSlices(PathBuf),
}

/// Discover ordered slice paths: single `.cci`, or `name.1.cci`, `name.2.cci`, …
/// when opened via `name.1.cci` (same rules as XboxToolkit `GetSlicesFromFile`, extended to multi-digit indices).
pub fn discover_slices(first_path: &Path) -> Result<Vec<PathBuf>, CciError> {
    if !first_path.is_file() {
        return Err(CciError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{:?} is not a file", first_path),
        )));
    }

    let ext = first_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if !ext.eq_ignore_ascii_case("cci") {
        return Err(CciError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "expected .cci extension",
        )));
    }

    let stem = first_path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| {
        CciError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid file name",
        ))
    })?;

    let dir = first_path.parent().unwrap_or(Path::new("."));

    if let Some((base, num)) = stem.rsplit_once('.') {
        if !base.is_empty() && !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
            let mut paths = Vec::new();
            for i in 1u32.. {
                let p = dir.join(format!("{base}.{i}.cci"));
                if p.is_file() {
                    paths.push(p);
                } else {
                    break;
                }
            }
            if !paths.is_empty() {
                return Ok(paths);
            }
        }
    }

    Ok(vec![first_path.to_path_buf()])
}

#[derive(Clone, Debug)]
pub struct CciHeader {
    pub uncompressed_size: u64,
    pub index_offset: u64,
}

fn parse_header(buf: &[u8; 32]) -> Result<CciHeader, CciError> {
    let magic = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    if magic != MAGIC {
        return Err(CciError::BadMagic);
    }
    let hdr_size = u32::from_le_bytes(buf[4..8].try_into().unwrap());
    if hdr_size != HEADER_SIZE as u32 {
        return Err(CciError::BadHeaderSize(hdr_size));
    }
    let uncompressed_size = u64::from_le_bytes(buf[8..16].try_into().unwrap());
    let index_offset = u64::from_le_bytes(buf[16..24].try_into().unwrap());
    let block_size = u32::from_le_bytes(buf[24..28].try_into().unwrap());
    if block_size != BLOCK_SIZE {
        return Err(CciError::BadBlockSize(block_size));
    }
    let version = buf[28];
    if version != 1 {
        return Err(CciError::BadVersion(version));
    }
    let index_align = buf[29];
    if index_align != INDEX_ALIGN as u8 {
        return Err(CciError::BadIndexAlign(index_align));
    }
    Ok(CciHeader {
        uncompressed_size,
        index_offset,
    })
}

fn write_placeholder_header(w: &mut File) -> Result<(), CciError> {
    let mut h = [0u8; 32];
    h[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    h[4..8].copy_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
    // uncompressed_size, index_offset left 0
    h[24..28].copy_from_slice(&BLOCK_SIZE.to_le_bytes());
    h[28] = 1;
    h[29] = INDEX_ALIGN as u8;
    w.write_all(&h)?;
    Ok(())
}

fn patch_header(w: &mut File, uncompressed_size: u64, index_offset: u64) -> Result<(), CciError> {
    w.seek(SeekFrom::Start(8))?;
    w.write_all(&uncompressed_size.to_le_bytes())?;
    w.write_all(&index_offset.to_le_bytes())?;
    Ok(())
}

#[inline]
fn index_file_offset(entry: u32) -> u64 {
    ((entry & 0x7fff_ffff) as u64) << INDEX_ALIGN
}

#[inline]
fn index_is_lz4(entry: u32) -> bool {
    entry & 0x8000_0000 != 0
}

/// Encode one 2048-byte sector to on-disk blob (XboxToolkit rules).
pub fn encode_sector(sector: &[u8; SECTOR_SIZE]) -> (Vec<u8>, bool) {
    let multiple = 1u32 << INDEX_ALIGN;
    let max_comp = get_maximum_output_size(SECTOR_SIZE);
    let mut compressed_buf = vec![0u8; max_comp];
    let compressed_len = match compress_into(sector, &mut compressed_buf) {
        Ok(n) => n,
        Err(_) => return (sector.to_vec(), false),
    };
    compressed_buf.truncate(compressed_len);

    let threshold = SECTOR_SIZE as i32 - (4 + multiple as i32);
    if compressed_len > 0 && (compressed_len as i32) < threshold {
        let pad_len =
            (((compressed_len + 1 + multiple as usize - 1) / multiple as usize) * multiple as usize)
                - (compressed_len + 1);
        let mut blob = Vec::with_capacity(1 + compressed_len + pad_len);
        blob.push(pad_len as u8);
        blob.extend_from_slice(&compressed_buf[..compressed_len]);
        blob.extend(std::iter::repeat(0u8).take(pad_len));
        return (blob, true);
    }

    (sector.to_vec(), false)
}

/// Decode one sector from a file span starting at `blob` (full index span).
pub fn decode_sector_blob(blob: &[u8], lz4: bool, out: &mut [u8; SECTOR_SIZE]) -> Result<(), CciError> {
    if !lz4 && blob.len() == SECTOR_SIZE {
        out.copy_from_slice(blob);
        return Ok(());
    }

    if blob.len() < 2 {
        return Err(CciError::BadSector(0));
    }
    let trail = blob[0] as usize;
    let comp_len = blob.len().saturating_sub(1).saturating_sub(trail);
    if comp_len < 1 || trail >= blob.len().saturating_sub(1) {
        return Err(CciError::BadSector(0));
    }
    let got = decompress_into(&blob[1..1 + comp_len], out)
        .map_err(|e| CciError::Lz4(e.to_string()))?;
    if got != SECTOR_SIZE {
        return Err(CciError::BadSector(0));
    }
    Ok(())
}

struct LoadedSlice {
    index: Vec<u32>,
    start_sector: u64,
    sectors: u64,
}

fn load_slice(path: &Path, start_sector: u64) -> Result<LoadedSlice, CciError> {
    let mut f = File::open(path)?;
    let mut hdr = [0u8; 32];
    f.read_exact(&mut hdr)?;
    let header = parse_header(&hdr)?;
    if header.uncompressed_size < SECTOR_SIZE as u64
        || header.uncompressed_size % SECTOR_SIZE as u64 != 0
    {
        return Err(CciError::BadSize);
    }
    let sectors = header.uncompressed_size / SECTOR_SIZE as u64;
    let entries = (sectors + 1) as usize;
    let index_byte_len = entries.checked_mul(4).ok_or(CciError::BadIndex)?;
    let mut raw = vec![0u8; index_byte_len];
    f.seek(SeekFrom::Start(header.index_offset))?;
    f.read_exact(&mut raw)?;
    let mut index = Vec::with_capacity(entries);
    for chunk in raw.chunks_exact(4) {
        index.push(u32::from_le_bytes(chunk.try_into().unwrap()));
    }
    if index.len() != entries {
        return Err(CciError::BadIndex);
    }

    Ok(LoadedSlice {
        index,
        start_sector,
        sectors,
    })
}

/// Random access to a (possibly split) CCI image.
pub struct CciImage {
    slices: Vec<LoadedSlice>,
    files: Vec<File>,
    total_sectors: u64,
}

impl CciImage {
    pub fn open(first_path: &Path) -> Result<Self, CciError> {
        let paths = discover_slices(first_path)?;
        if paths.is_empty() {
            return Err(CciError::NoSlices(first_path.to_path_buf()));
        }

        let mut slices = Vec::new();
        let mut files = Vec::new();
        let mut global = 0u64;

        for p in &paths {
            let s = load_slice(p, global)?;
            global += s.sectors;
            let f = File::open(p)?;
            files.push(f);
            slices.push(s);
        }

        Ok(Self {
            total_sectors: global,
            slices,
            files,
        })
    }

    pub fn total_sectors(&self) -> u64 {
        self.total_sectors
    }

    pub fn uncompressed_size(&self) -> u64 {
        self.total_sectors * SECTOR_SIZE as u64
    }

    /// Read global LBA `sector` into `out`.
    pub fn read_sector(&mut self, sector: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), CciError> {
        let (si, local) = self
            .slices
            .iter()
            .enumerate()
            .find_map(|(i, s)| {
                if sector >= s.start_sector && sector < s.start_sector + s.sectors {
                    Some((i, sector - s.start_sector))
                } else {
                    None
                }
            })
            .ok_or(CciError::BadSector(sector))?;

        let s = &self.slices[si];
        let li = local as usize;
        if li + 1 >= s.index.len() {
            return Err(CciError::BadSector(sector));
        }

        let e0 = s.index[li];
        let e1 = s.index[li + 1];
        let pos0 = index_file_offset(e0);
        let pos1 = index_file_offset(e1);
        let lz4 = index_is_lz4(e0);
        if pos1 < pos0 || pos1 - pos0 > 32 * 1024 * 1024 {
            return Err(CciError::BadSector(sector));
        }
        let span = (pos1 - pos0) as usize;

        let f = &mut self.files[si];
        f.seek(SeekFrom::Start(pos0))?;
        let mut buf = vec![0u8; span];
        f.read_exact(&mut buf)?;

        decode_sector_blob(&buf, lz4, out).map_err(|_| CciError::BadSector(sector))
    }
}

/// Write a raw sector stream (e.g. ISO) to CCI, optionally splitting at `split_point` bytes (0 = no split).
/// Naming matches XboxToolkit: output `stem.cci` if a single part; otherwise `stem.1.cci`, `stem.2.cci`, …
pub fn iso_to_cci(
    mut input: impl Read,
    input_len: u64,
    output_path: &Path,
    split_point: u64,
) -> Result<(), CciError> {
    if input_len == 0 {
        return Err(CciError::BadSize);
    }
    let total_sectors = input_len.div_ceil(SECTOR_SIZE as u64);

    let dir = output_path.parent().unwrap_or(Path::new("."));
    let ext = output_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("cci");
    let stem = output_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("out");

    let mut sector_buf = [0u8; SECTOR_SIZE];
    let mut global_sector: u64 = 0;
    let mut iteration: u32 = 0;

    while global_sector < total_sectors {
        let mut w = File::create(output_path)?;
        write_placeholder_header(&mut w)?;

        let mut blobs: Vec<(u64, bool)> = Vec::new();
        let mut slice_sectors: u64 = 0;
        let mut splitting = false;

        while global_sector < total_sectors {
            let pos = w.stream_position()?;
            let idx_bytes = blobs.len() as u64 * 4;
            let estimated = pos + idx_bytes + SECTOR_SIZE as u64;
            if split_point > 0 && estimated > split_point && slice_sectors > 0 {
                splitting = true;
                break;
            }

            let n = input.read(&mut sector_buf)?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "ISO ended before expected sector count",
                )
                .into());
            }
            if n < SECTOR_SIZE {
                sector_buf[n..].fill(0);
            }

            let (blob, compressed) = encode_sector(&sector_buf);
            w.write_all(&blob)?;
            blobs.push((blob.len() as u64, compressed));
            slice_sectors += 1;
            global_sector += 1;
        }

        if blobs.is_empty() {
            drop(w);
            std::fs::remove_file(output_path).ok();
            break;
        }

        let uncompressed_size = slice_sectors * SECTOR_SIZE as u64;
        let index_offset = w.stream_position()?;

        let mut position = HEADER_SIZE;
        for (len, is_lz4) in &blobs {
            let word = ((position >> INDEX_ALIGN) as u32) | if *is_lz4 { 0x8000_0000 } else { 0 };
            w.write_all(&word.to_le_bytes())?;
            position += *len;
        }
        let end_word = (position >> INDEX_ALIGN) as u32;
        w.write_all(&end_word.to_le_bytes())?;

        patch_header(&mut w, uncompressed_size, index_offset)?;
        drop(w);

        if splitting || iteration > 0 {
            let dest = dir.join(format!("{stem}.{}.{}", iteration + 1, ext));
            if dest.exists() {
                std::fs::remove_file(&dest)?;
            }
            std::fs::rename(output_path, &dest)?;
        }

        iteration += 1;
        if !splitting {
            break;
        }
    }

    Ok(())
}

/// Decode full image to a raw sector stream (ISO).
pub fn cci_to_iso(first_path: &Path, mut output: impl Write) -> Result<(), CciError> {
    let mut img = CciImage::open(first_path)?;
    let mut sec = [0u8; SECTOR_SIZE];
    for i in 0..img.total_sectors() {
        img.read_sector(i, &mut sec)?;
        output.write_all(&sec)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sector_roundtrip_zeros() {
        let mut z = [0u8; SECTOR_SIZE];
        let (blob, lz) = encode_sector(&z);
        let mut out = [0u8; SECTOR_SIZE];
        decode_sector_blob(&blob, lz, &mut out).unwrap();
        assert_eq!(z, out);
    }

    #[test]
    fn sector_roundtrip_pattern() {
        let mut s = [0u8; SECTOR_SIZE];
        for (i, b) in s.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        let (blob, lz) = encode_sector(&s);
        let mut out = [0u8; SECTOR_SIZE];
        decode_sector_blob(&blob, lz, &mut out).unwrap();
        assert_eq!(s, out);
    }

    #[test]
    fn tiny_iso_roundtrip_file() -> Result<(), CciError> {
        let tmp = std::env::temp_dir().join("xdvdfs_cci_test.iso");
        let cci = std::env::temp_dir().join("xdvdfs_cci_test.cci");
        let data = vec![0xABu8; SECTOR_SIZE * 3 + 100];
        std::fs::write(&tmp, &data)?;
        let len = data.len() as u64;
        let mut f = File::open(&tmp)?;
        iso_to_cci(&mut f, len, &cci, 0)?;
        let mut decoded = Vec::new();
        cci_to_iso(&cci, &mut decoded)?;
        assert_eq!(decoded.len(), 4 * SECTOR_SIZE);
        assert_eq!(&decoded[..data.len()], &data[..]);
        assert!(decoded[data.len()..].iter().all(|&b| b == 0));
        std::fs::remove_file(&tmp).ok();
        std::fs::remove_file(&cci).ok();
        Ok(())
    }

    #[test]
    fn split_naming() -> Result<(), CciError> {
        let dir = std::env::temp_dir().join("xdvdfs_cci_split");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let iso_path = dir.join("t.iso");
        let cci_base = dir.join("game.cci");
        let data = vec![0xCDu8; SECTOR_SIZE * 20];
        std::fs::write(&iso_path, &data)?;
        let mut f = File::open(&iso_path)?;
        // Force split early (see XboxToolkit split check: pos + index*4 + 2048)
        iso_to_cci(&mut f, data.len() as u64, &cci_base, 2100)?;
        assert!(!cci_base.exists());
        assert!(dir.join("game.1.cci").is_file());
        assert!(dir.join("game.2.cci").is_file());
        let mut out = Vec::new();
        cci_to_iso(&dir.join("game.1.cci"), &mut out)?;
        assert_eq!(out.len(), data.len());
        assert_eq!(out, data);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
