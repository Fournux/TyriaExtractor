use anyhow::{Context, Result, bail};
use std::{collections::BTreeMap, path::Path};

use crate::{
    capture::{CaptureDomain, CaptureManifest, analyze_capture},
    io_util::{StagedDirectory, write_json},
    items, quests, skills,
};

pub(crate) fn extract_skills(snapshot: &Path, output_root: &Path) -> Result<()> {
    let final_dir = output_root.join("skills");
    let staging = StagedDirectory::new(&final_dir)?;
    let out_dir = staging.path();
    let skills_json = out_dir.join("skills.json");
    let model_file_dir = out_dir.join("model_file");
    let model_file_hd_dir = out_dir.join("model_file_hd");
    skills::extract_skills_to_model_file_dirs(
        snapshot,
        &skills_json,
        &model_file_dir,
        &model_file_hd_dir,
    )
    .with_context(|| format!("extracting skills from {}", snapshot.display()))?;
    staging.commit()
}

pub(crate) fn extract_items(
    snapshot: &Path,
    packet_log: Option<&Path>,
    skip_icons: bool,
    use_client_strings: bool,
    allow_unverified_capture: bool,
    output_root: &Path,
) -> Result<()> {
    if skip_icons && packet_log.is_none() {
        bail!("item extraction has no work: --skip-icons requires --packet-log");
    }
    let final_dir = output_root.join("items");
    let staging = StagedDirectory::new(&final_dir)?;
    let out_dir = staging.path();
    let model_file_dir = out_dir.join("model_file");
    let capture_report = packet_log
        .map(|path| {
            let report = analyze_capture(path, CaptureDomain::General)
                .with_context(|| format!("validating item capture {}", path.display()))?;
            if !allow_unverified_capture {
                report.ensure_verified(path)?;
            }
            Ok::<_, anyhow::Error>(report)
        })
        .transpose()?;
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
    if let Some(report) = capture_report {
        write_json(
            &out_dir.join("capture.json"),
            &CaptureManifest::new(BTreeMap::from([("items".to_string(), report)])),
        )?;
    }
    staging.commit()
}

pub(crate) fn extract_quests(
    snapshot: &Path,
    packet_log: &Path,
    item_log: Option<&Path>,
    allow_unverified_capture: bool,
    output_root: &Path,
) -> Result<()> {
    let quest_report = analyze_capture(packet_log, CaptureDomain::Quest)
        .with_context(|| format!("validating quest capture {}", packet_log.display()))?;
    if !allow_unverified_capture {
        quest_report.ensure_verified(packet_log)?;
    }
    let mut captures = BTreeMap::from([("quests".to_string(), quest_report)]);
    if let Some(item_log) = item_log {
        let item_report = analyze_capture(item_log, CaptureDomain::General)
            .with_context(|| format!("validating reward item capture {}", item_log.display()))?;
        if !allow_unverified_capture {
            item_report.ensure_verified(item_log)?;
        }
        captures.insert("reward_items".to_string(), item_report);
    }
    let staging = StagedDirectory::new(&output_root.join("quests"))?;
    let out_dir = staging.path();
    quests::extract_quests_from_packet_log(
        snapshot,
        packet_log,
        item_log,
        &out_dir.join("quests.json"),
    )
    .with_context(|| format!("extracting quests from {}", packet_log.display()))?;
    write_json(
        &out_dir.join("capture.json"),
        &CaptureManifest::new(captures),
    )?;
    staging.commit()
}
