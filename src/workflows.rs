use anyhow::{Context, Result};
use std::{fs, path::Path};

use crate::{items, quests, skills};

pub(crate) fn extract_skills(snapshot: &Path) -> Result<()> {
    let out_dir = Path::new("skills");
    fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    let skills_json = out_dir.join("skills.json");
    let model_file_dir = out_dir.join("model_file");
    let model_file_hd_dir = out_dir.join("model_file_hd");
    skills::extract_skills_to_model_file_dirs(
        snapshot,
        &skills_json,
        &model_file_dir,
        &model_file_hd_dir,
    )
    .with_context(|| format!("extracting skills from {}", snapshot.display()))
}

pub(crate) fn extract_items(
    snapshot: &Path,
    packet_log: Option<&Path>,
    skip_icons: bool,
    use_client_strings: bool,
) -> Result<()> {
    let out_dir = Path::new("items");
    let model_file_dir = out_dir.join("model_file");

    if !skip_icons {
        items::export_model_file_icons(snapshot, &model_file_dir, 1, None, false, None)
            .with_context(|| format!("extracting item model icons from {}", snapshot.display()))?;
    }
    if let Some(packet_log) = packet_log {
        let text_inputs = items::packet_log_text_inputs(packet_log, use_client_strings)
            .with_context(|| format!("reading item text inputs from {}", packet_log.display()))?;
        let names = items::runtime_item_text_lookup_with_compact_seeds(
            snapshot,
            &text_inputs.name_ids,
            &text_inputs.compact_seeds,
            &text_inputs.decoded_records,
        )
        .with_context(|| format!("resolving item names from {}", snapshot.display()))?;
        items::export_detected_items_from_packet_log_with_client_strings(
            packet_log,
            &names,
            &out_dir.join("items.json"),
            use_client_strings,
        )
        .with_context(|| format!("extracting runtime items from {}", packet_log.display()))?;
    }
    Ok(())
}

pub(crate) fn extract_quests(
    snapshot: &Path,
    packet_log: &Path,
    item_log: Option<&Path>,
) -> Result<()> {
    quests::extract_quests_from_packet_log(
        snapshot,
        packet_log,
        item_log,
        &Path::new("quests").join("quests.json"),
    )
    .with_context(|| format!("extracting quests from {}", packet_log.display()))
}
