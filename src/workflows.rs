use anyhow::{Context, Result, bail};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use crate::{
    capture::{CaptureDomain, CaptureManifest, analyze_capture},
    images,
    io_util::{StagedDirectory, write_json},
    items, npcs, quests, skills, vendors,
};
const CAPTURE_METADATA_JSONL_FILENAME: &str = "tyria_capture.jsonl";

fn merge_capture_inputs(inputs: &[PathBuf], staging: &Path, label: &str) -> Result<PathBuf> {
    if inputs.is_empty() {
        bail!("at least one {label} capture is required");
    }
    let metadata_paths = inputs
        .iter()
        .map(|input| input.with_file_name(CAPTURE_METADATA_JSONL_FILENAME))
        .collect::<BTreeSet<_>>();
    if let Some(path) = metadata_paths.iter().find(|path| !path.is_file()) {
        bail!("missing capture metadata sidecar {}", path.display());
    }

    let merged_path = staging.join(format!(".{label}-merged.jsonl"));
    let file = File::create(&merged_path)
        .with_context(|| format!("creating {}", merged_path.display()))?;
    let mut rows = Vec::new();
    for (input_index, input_path) in inputs.iter().chain(&metadata_paths).enumerate() {
        let input =
            File::open(input_path).with_context(|| format!("opening {}", input_path.display()))?;
        for (line_index, line) in BufReader::new(input).lines().enumerate() {
            let line = line.with_context(|| {
                format!("reading {} line {}", input_path.display(), line_index + 1)
            })?;
            if line.trim().is_empty() {
                continue;
            }
            let value: serde_json::Value = serde_json::from_str(&line).with_context(|| {
                format!("decoding {} line {}", input_path.display(), line_index + 1)
            })?;
            let kind = value.get("kind").and_then(serde_json::Value::as_str);
            let is_metadata = matches!(
                kind,
                Some("world_packet_schema" | "status" | "capture_health")
            );
            if input_index < inputs.len() && is_metadata {
                bail!(
                    "capture metadata at {} line {} belongs in {}",
                    input_path.display(),
                    line_index + 1,
                    CAPTURE_METADATA_JSONL_FILENAME
                );
            }
            if input_index >= inputs.len() && !is_metadata {
                bail!(
                    "capture data at {} line {} does not belong in {}",
                    input_path.display(),
                    line_index + 1,
                    CAPTURE_METADATA_JSONL_FILENAME
                );
            }
            let session_id = value
                .get("session_id")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            let capture_seq = value.get("capture_seq").and_then(serde_json::Value::as_u64);
            let (order_class, order_value) = if kind == Some("world_packet_schema") {
                (0, 0)
            } else if let Some(capture_seq) = capture_seq {
                (1, capture_seq)
            } else {
                (
                    2,
                    value
                        .get("ts_ms")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or_default(),
                )
            };
            rows.push((
                session_id,
                order_class,
                order_value,
                input_index,
                line_index,
                line,
            ));
        }
    }
    rows.sort_by_key(|row| (row.0, row.1, row.2, row.3, row.4));
    let mut output = BufWriter::new(file);
    for (_, _, _, _, _, line) in rows {
        writeln!(output, "{line}")?;
    }
    output.flush()?;
    Ok(merged_path)
}

fn remove_merged_capture(path: &Path) -> Result<()> {
    fs::remove_file(path).with_context(|| format!("removing {}", path.display()))
}

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

pub(crate) fn extract_images(snapshot: &Path, output_root: &Path) -> Result<()> {
    images::extract_all_images(snapshot, &output_root.join("images"))
        .with_context(|| format!("extracting images from {}", snapshot.display()))
}

pub(crate) fn extract_items(
    snapshot: &Path,
    packet_logs: &[PathBuf],
    skip_icons: bool,
    use_client_strings: bool,
    output_root: &Path,
) -> Result<()> {
    if skip_icons && packet_logs.is_empty() {
        bail!("item extraction has no work: --skip-icons requires --packet-log");
    }
    let final_dir = output_root.join("items");
    let staging = StagedDirectory::new(&final_dir)?;
    let out_dir = staging.path();
    let model_file_dir = out_dir.join("model_file");
    let packet_log = (!packet_logs.is_empty())
        .then(|| merge_capture_inputs(packet_logs, out_dir, "items"))
        .transpose()?;
    let capture_report = packet_log
        .as_deref()
        .map(|path| {
            let report = analyze_capture(path, CaptureDomain::General)
                .with_context(|| format!("validating item capture {}", path.display()))?;
            report.ensure_verified(path)?;
            Ok::<_, anyhow::Error>(report)
        })
        .transpose()?;
    if !skip_icons {
        items::export_model_file_icons(snapshot, &model_file_dir, 1, None, false, None)
            .with_context(|| format!("extracting item model icons from {}", snapshot.display()))?;
    }
    if let Some(packet_log) = packet_log.as_deref() {
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
    if let Some(packet_log) = &packet_log {
        remove_merged_capture(packet_log)?;
    }
    staging.commit()
}

pub(crate) fn extract_quests(
    snapshot: &Path,
    packet_logs: &[PathBuf],
    item_logs: &[PathBuf],
    output_root: &Path,
) -> Result<()> {
    let staging = StagedDirectory::new(&output_root.join("quests"))?;
    let out_dir = staging.path();
    let packet_log = merge_capture_inputs(packet_logs, out_dir, "quests")?;
    let item_log = (!item_logs.is_empty())
        .then(|| merge_capture_inputs(item_logs, out_dir, "quest-items"))
        .transpose()?;
    let quest_report = analyze_capture(packet_log.as_ref(), CaptureDomain::World)
        .with_context(|| format!("validating quest capture {}", packet_log.display()))?;
    quest_report.ensure_verified(packet_log.as_ref())?;
    let mut captures = BTreeMap::from([("quests".to_string(), quest_report)]);
    if let Some(item_log) = item_log.as_deref() {
        let item_report = analyze_capture(item_log, CaptureDomain::General)
            .with_context(|| format!("validating reward item capture {}", item_log.display()))?;
        item_report.ensure_verified(item_log)?;
        captures.insert("reward_items".to_string(), item_report);
    }
    quests::extract_quests_from_packet_log(
        snapshot,
        packet_log.as_ref(),
        item_log.as_deref(),
        &out_dir.join("quests.json"),
    )
    .with_context(|| format!("extracting quests from {}", packet_log.display()))?;
    write_json(
        &out_dir.join("capture.json"),
        &CaptureManifest::new(captures),
    )?;
    remove_merged_capture(&packet_log)?;
    if let Some(item_log) = &item_log {
        remove_merged_capture(item_log)?;
    }
    staging.commit()
}

pub(crate) fn extract_npcs(
    snapshot: &Path,
    packet_logs: &[PathBuf],
    output_root: &Path,
) -> Result<()> {
    let staging = StagedDirectory::new(&output_root.join("npcs"))?;
    let out_dir = staging.path();
    let packet_log = merge_capture_inputs(packet_logs, out_dir, "npcs")?;
    let report = analyze_capture(packet_log.as_ref(), CaptureDomain::World)
        .with_context(|| format!("validating NPC capture {}", packet_log.display()))?;
    report.ensure_verified(packet_log.as_ref())?;
    npcs::extract_npcs_from_packet_log(snapshot, packet_log.as_ref(), &out_dir.join("npcs.json"))
        .with_context(|| format!("extracting NPCs from {}", packet_log.display()))?;
    write_json(
        &out_dir.join("capture.json"),
        &CaptureManifest::new(BTreeMap::from([("npcs".to_string(), report)])),
    )?;
    remove_merged_capture(&packet_log)?;
    staging.commit()
}

pub(crate) fn extract_vendors(
    snapshot: &Path,
    packet_logs: &[PathBuf],
    output_root: &Path,
) -> Result<()> {
    let staging = StagedDirectory::new(&output_root.join("vendors"))?;
    let out_dir = staging.path();
    let packet_log = merge_capture_inputs(packet_logs, out_dir, "vendors")?;
    let report = analyze_capture(packet_log.as_ref(), CaptureDomain::World)
        .with_context(|| format!("validating vendor capture {}", packet_log.display()))?;
    report.ensure_verified(packet_log.as_ref())?;
    let encoded_names = vendors::captured_npc_name_words(packet_log.as_ref())
        .with_context(|| format!("reading vendor NPC names from {}", packet_log.display()))?;
    let localized_npc_names = npcs::resolve_localized_npc_names(snapshot, &encoded_names)
        .with_context(|| format!("resolving vendor NPC names from {}", snapshot.display()))?;
    vendors::extract_vendor_catalogs_from_packet_log(
        packet_log.as_ref(),
        out_dir,
        &localized_npc_names,
    )
    .with_context(|| format!("extracting vendor catalogs from {}", packet_log.display()))?;
    write_json(
        &out_dir.join("capture.json"),
        &CaptureManifest::new(BTreeMap::from([("vendors".to_string(), report)])),
    )?;
    remove_merged_capture(&packet_log)?;
    staging.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn merges_capture_files_by_capture_sequence() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "tyria-capture-merge-{}-{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root)?;
        let first = root.join("quests.jsonl");
        let second = root.join("npcs.jsonl");
        fs::write(
            &first,
            "{\"kind\":\"world_packet\",\"session_id\":1,\"capture_seq\":2}\n",
        )?;
        fs::write(
            &second,
            "{\"kind\":\"world_packet\",\"session_id\":1,\"capture_seq\":1}\n",
        )?;
        fs::write(
            root.join(CAPTURE_METADATA_JSONL_FILENAME),
            concat!(
                "{\"kind\":\"world_packet_schema\",\"session_id\":1}\n",
                "{\"kind\":\"capture_health\",\"session_id\":1,\"ts_ms\":3}\n"
            ),
        )?;

        let inputs = [first, second];
        let merged = merge_capture_inputs(&inputs, &root, "test")?;
        assert_eq!(
            fs::read_to_string(&merged)?,
            concat!(
                "{\"kind\":\"world_packet_schema\",\"session_id\":1}\n",
                "{\"kind\":\"world_packet\",\"session_id\":1,\"capture_seq\":1}\n",
                "{\"kind\":\"world_packet\",\"session_id\":1,\"capture_seq\":2}\n",
                "{\"kind\":\"capture_health\",\"session_id\":1,\"ts_ms\":3}\n"
            )
        );
        remove_merged_capture(&merged)?;
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn rejects_capture_without_current_metadata_sidecar() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "tyria-capture-current-{}-{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root)?;
        let input = root.join("items.jsonl");
        fs::write(&input, "{}\n")?;
        fs::write(root.join("tyria_capture.json"), "{}\n")?;

        let error = merge_capture_inputs(std::slice::from_ref(&input), &root, "test")
            .expect_err("old metadata sidecar must be rejected");
        assert!(error.to_string().contains("tyria_capture.jsonl"));

        fs::write(
            root.join(CAPTURE_METADATA_JSONL_FILENAME),
            "{\"kind\":\"capture_health\",\"session_id\":1,\"capture_format_version\":5}\n",
        )?;
        fs::write(
            &input,
            "{\"kind\":\"world_packet_schema\",\"session_id\":1}\n",
        )?;
        let error = merge_capture_inputs(std::slice::from_ref(&input), &root, "test")
            .expect_err("inline metadata must be rejected");
        assert!(error.to_string().contains("belongs in tyria_capture.jsonl"));
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
