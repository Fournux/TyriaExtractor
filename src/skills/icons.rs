use std::path::Path;

use crate::{atex, dat::DatArchive};

pub(super) fn export_skill_icon(
    archive: &mut DatArchive,
    texture_hash: u32,
    out_path: &Path,
) -> anyhow::Result<bool> {
    let Some(mft_index) = archive.mft_index_for_file_id(texture_hash) else {
        return Ok(false);
    };
    let bytes = archive.read_entry(mft_index)?;
    atex::save_atex_as_png(&bytes, out_path)?;
    Ok(true)
}
