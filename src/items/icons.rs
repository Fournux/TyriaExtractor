use anyhow::{Context, Result, bail};
use std::{collections::BTreeSet, fs, path::Path};

use crate::{
    dat::{DatArchive, lookup_mft_stream_entry_from_base},
    icon_payload::decode_icon_payload,
    io_util::write_json,
};

fn relative_entry_path(mft_entry_index: u32) -> String {
    format!("{:03}/{:06}.bin", mft_entry_index / 1000, mft_entry_index)
}

#[cfg(test)]
pub(crate) use crate::icon_payload::find_inline_atex_payload;

struct ModelFileIconCandidate<'a> {
    source: &'static str,
    stream_id: Option<u8>,
    image_entry: &'a crate::models::MftEntry,
}

fn model_file_icon_candidate_for_base<'a>(
    base_entry: &'a crate::models::MftEntry,
    include_direct: bool,
    mft_entries: &'a [crate::models::MftEntry],
) -> Option<ModelFileIconCandidate<'a>> {
    if let Some(stream_entry) = lookup_mft_stream_entry_from_base(base_entry, 1, mft_entries) {
        return Some(ModelFileIconCandidate {
            source: "model_stream_1",
            stream_id: Some(1),
            image_entry: stream_entry,
        });
    }

    include_direct.then_some(ModelFileIconCandidate {
        source: "direct_file",
        stream_id: None,
        image_entry: base_entry,
    })
}

#[cfg(test)]
pub(crate) fn model_file_icon_candidate_for_test(
    base_entry: &crate::models::MftEntry,
    include_direct: bool,
    mft_entries: &[crate::models::MftEntry],
) -> Option<(&'static str, Option<u8>, u32)> {
    model_file_icon_candidate_for_base(base_entry, include_direct, mft_entries).map(|candidate| {
        (
            candidate.source,
            candidate.stream_id,
            candidate.image_entry.index,
        )
    })
}

struct ModelFileIconPayloadContext<'a> {
    model_file_id: u32,
    source: &'a str,
    stream_id: Option<u8>,
    base_entry: &'a crate::models::MftEntry,
    image_entry: &'a crate::models::MftEntry,
    base_hashes: &'a [u32],
    image_hashes: &'a [u32],
    out_dir: &'a Path,
}

fn export_model_file_icon_payload(
    context: ModelFileIconPayloadContext<'_>,
    bytes: &[u8],
) -> Result<Option<serde_json::Value>> {
    let Some(payload) = decode_icon_payload(bytes)? else {
        return Ok(None);
    };
    let filename = format!("{}.png", context.model_file_id);
    let png_path = context.out_dir.join(&filename);
    payload.save_png(&png_path)?;

    let mut value = serde_json::json!({
        "model_file_id": context.model_file_id,
        "source": context.source,
        "stream_id": context.stream_id,
        "base_mft_entry_index": context.base_entry.index,
        "base_hashes": context.base_hashes,
        "base_relative_path": relative_entry_path(context.base_entry.index),
        "image_mft_entry_index": context.image_entry.index,
        "image_hashes": context.image_hashes,
        "image_relative_path": relative_entry_path(context.image_entry.index),
        "kind": payload.kind(),
        "width": payload.width(),
        "height": payload.height(),
        "format": payload.format(),
        "png": filename,
    });
    if let Some(offset) = payload.inline_texture_offset() {
        value["inline_texture_offset"] = serde_json::Value::from(offset);
    }
    Ok(Some(value))
}

#[cfg(test)]
#[expect(
    clippy::too_many_arguments,
    reason = "test shim preserves concise fixtures"
)]
pub(crate) fn export_model_file_icon_payload_for_test(
    model_file_id: u32,
    source: &str,
    stream_id: Option<u8>,
    base_entry: &crate::models::MftEntry,
    image_entry: &crate::models::MftEntry,
    base_hashes: &[u32],
    image_hashes: &[u32],
    bytes: &[u8],
    out_dir: &Path,
) -> Result<Option<serde_json::Value>> {
    fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    export_model_file_icon_payload(
        ModelFileIconPayloadContext {
            model_file_id,
            source,
            stream_id,
            base_entry,
            image_entry,
            base_hashes,
            image_hashes,
            out_dir,
        },
        bytes,
    )
}

pub(crate) fn export_model_file_icons(
    gw_dat_path: &Path,
    out_dir: &Path,
    start_id: u32,
    max_id: Option<u32>,
    include_direct: bool,
    limit: Option<usize>,
) -> Result<()> {
    if start_id == 0 {
        bail!("start_id must be >= 1");
    }
    if let Some(max_id) = max_id
        && start_id > max_id
    {
        bail!("start_id ({start_id}) must be <= max_id ({max_id})");
    }

    let mut archive = DatArchive::open(gw_dat_path)?;
    let model_file_ids = archive
        .hash_lookup()
        .iter()
        .map(|entry| entry.file_number)
        .filter(|file_id| *file_id >= start_id && max_id.is_none_or(|max_id| *file_id <= max_id))
        .collect::<BTreeSet<_>>();
    let effective_max_id = max_id.or_else(|| model_file_ids.iter().next_back().copied());
    let range_label = match effective_max_id {
        Some(max_id) => format!("{start_id}..={max_id}"),
        None => format!("{start_id}..=<none>"),
    };

    println!(
        "Exporting model-file stream-1 icons from {} for hash aliases in {}",
        gw_dat_path.display(),
        range_label
    );
    if include_direct {
        println!("Including direct image payload fallback for ids without stream 1");
    }

    fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;

    let total_candidate_model_file_ids = model_file_ids.len();
    let mut scanned_model_file_ids = 0_u64;
    let mut missing_mft_entries = 0_u64;
    let mut stream1_candidates = 0_u64;
    let mut skipped_without_stream1 = 0_u64;
    let mut direct_file_candidates = 0_u64;
    let mut unsupported_payloads = 0_u64;
    let mut failed_payloads = 0_u64;
    let mut exported_icons = 0_usize;
    let mut manifest_entries = Vec::new();

    for model_file_id in model_file_ids {
        scanned_model_file_ids += 1;
        let Some(base_mft_index) = archive.mft_index_for_file_id(model_file_id) else {
            continue;
        };
        let Some(base_entry) = archive.entry(base_mft_index) else {
            missing_mft_entries += 1;
            continue;
        };

        let Some(candidate) =
            model_file_icon_candidate_for_base(&base_entry, include_direct, archive.entries())
        else {
            skipped_without_stream1 += 1;
            continue;
        };
        if candidate.stream_id == Some(1) {
            stream1_candidates += 1;
        } else {
            direct_file_candidates += 1;
        }
        let source = candidate.source;
        let stream_id = candidate.stream_id;
        let image_entry = *candidate.image_entry;

        let result = archive
            .read_entry(image_entry.index)
            .with_context(|| format!("reading {source} MFT entry {}", image_entry.index));
        let result = match result {
            Ok(bytes) => {
                let base_hashes = archive
                    .hashes_by_mft()
                    .get(&base_entry.index)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let image_hashes = archive
                    .hashes_by_mft()
                    .get(&image_entry.index)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                export_model_file_icon_payload(
                    ModelFileIconPayloadContext {
                        model_file_id,
                        source,
                        stream_id,
                        base_entry: &base_entry,
                        image_entry: &image_entry,
                        base_hashes,
                        image_hashes,
                        out_dir,
                    },
                    &bytes,
                )
            }
            Err(error) => Err(error),
        };

        match result {
            Ok(Some(value)) => {
                manifest_entries.push(value);
                exported_icons += 1;
            }
            Ok(None) => {
                unsupported_payloads += 1;
            }
            Err(err) => {
                failed_payloads += 1;
                manifest_entries.push(serde_json::json!({
                    "model_file_id": model_file_id,
                    "source": source,
                    "stream_id": stream_id,
                    "base_mft_entry_index": base_entry.index,
                    "image_mft_entry_index": image_entry.index,
                    "error": format!("{err:#}"),
                }));
            }
        }

        if limit.is_some_and(|limit| exported_icons >= limit) {
            break;
        }
    }

    let manifest = serde_json::json!({
        "schema_version": 1,
        "note": "Model-file icon export keyed by DAT hash aliases. By default this decodes only linked stream 1, matching GWToolbox++ item UI icon loading and avoiding direct skill-icon texture references. Use include_direct only for diagnostic standalone ATEX/ATTX/DDS/inline-FFNA payloads; that mode is mixed and can include skill/UI textures. PNG filenames are <model_file_id>.png.",
        "start_id": start_id,
        "max_id": max_id,
        "effective_max_id": effective_max_id,
        "include_direct": include_direct,
        "counts": {
            "candidate_hash_aliases": total_candidate_model_file_ids,
            "scanned_model_file_ids": scanned_model_file_ids,
            "missing_mft_entries": missing_mft_entries,
            "stream1_candidates": stream1_candidates,
            "skipped_without_stream1": skipped_without_stream1,
            "direct_file_candidates": direct_file_candidates,
            "exported_icons": exported_icons,
            "unsupported_payloads": unsupported_payloads,
            "failed_payloads": failed_payloads,
        },
        "entries": manifest_entries,
    });
    write_json(&out_dir.join("manifest.json"), &manifest)?;
    println!(
        "Exported {} model-file icons (skipped without stream 1: {}, unsupported: {}, failures: {})",
        exported_icons, skipped_without_stream1, unsupported_payloads, failed_payloads
    );
    Ok(())
}
