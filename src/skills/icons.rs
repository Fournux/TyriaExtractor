use std::{collections::BTreeMap, fs::File, path::Path};

use crate::{
    atex,
    dat::{lookup_mft_index_for_file_id, mft_entry_by_index, read_dat_entry_from_file},
    models::MftEntry,
};

pub(super) fn export_skill_icon(
    file: &mut File,
    file_size: u64,
    texture_hash: u32,
    hash_to_mft: &BTreeMap<u32, u32>,
    mft_entries: &[MftEntry],
    out_path: &Path,
) {
    let Some(mft_index) = lookup_mft_index_for_file_id(texture_hash, hash_to_mft) else {
        return;
    };
    let Some(icon_entry) = mft_entry_by_index(mft_entries, mft_index) else {
        return;
    };
    let _ = read_dat_entry_from_file(file, file_size, icon_entry)
        .and_then(|bytes| atex::save_atex_as_png(&bytes, out_path));
}
