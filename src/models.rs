use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub(crate) struct GwDatHeader {
    pub(crate) magic_hex: String,
    pub(crate) version: u8,
    pub(crate) header_size: u32,
    pub(crate) sector_size: u32,
    pub(crate) crc_hex: String,
    pub(crate) mft_offset: u64,
    pub(crate) mft_size: u32,
    pub(crate) flags: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct MftHeader {
    pub(crate) magic_hex: String,
    pub(crate) entry_count: u32,
    pub(crate) unknown_1: u32,
    pub(crate) unknown_2: u32,
    pub(crate) unknown_4: u32,
    pub(crate) unknown_5: u32,
}

#[derive(Debug)]
pub(crate) struct MftEntry {
    pub(crate) index: u32,
    pub(crate) offset: u64,
    pub(crate) size: u32,
    pub(crate) compression: u16,
    pub(crate) content: u8,
    pub(crate) content_type: u8,
    pub(crate) id: u32,
    pub(crate) crc: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HashLookupEntry {
    pub(crate) file_number: u32,
    pub(crate) mft_index: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct DatCacheManifest {
    pub(crate) generated_at: DateTime<Utc>,
    pub(crate) gw_dat_path: PathBuf,
    pub(crate) gw_dat_cache_key: String,
    pub(crate) cache_root: PathBuf,
    pub(crate) header: GwDatHeader,
    pub(crate) mft: MftHeader,
    pub(crate) active_entries: usize,
    pub(crate) dumped_entries: usize,
    pub(crate) decompressed_entries: usize,
    pub(crate) failed_entries: usize,
    pub(crate) entries: Vec<DatCacheEntry>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DatCacheEntry {
    pub(crate) index: u32,
    pub(crate) offset: u64,
    pub(crate) compressed_size: u32,
    pub(crate) decompressed_size: usize,
    pub(crate) compression: u16,
    pub(crate) content: u8,
    pub(crate) content_type: u8,
    pub(crate) id: u32,
    pub(crate) crc_hex: String,
    pub(crate) hashes: Vec<u32>,
    pub(crate) magic_hex: String,
    pub(crate) kind: String,
    pub(crate) utf16le_string_count: usize,
    pub(crate) relative_path: PathBuf,
}
