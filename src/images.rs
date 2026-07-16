use anyhow::{Context, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use crate::{
    dat::DatArchive,
    icon_payload::{IconPayload, decode_icon_payload, find_inline_atex_payloads},
    io_util::write_json,
};

pub(crate) fn extract_all_images(gw_dat_path: &Path, out_dir: &Path) -> Result<()> {
    let mut archive = DatArchive::open(gw_dat_path)?;
    let file_ids_by_mft = file_ids_by_mft_chain(&archive);
    let entry_count = archive
        .entries()
        .iter()
        .filter(|entry| entry.size != 0 && entry.content != 0)
        .count();

    if out_dir.exists() {
        fs::remove_dir_all(out_dir)
            .with_context(|| format!("removing previous image export {}", out_dir.display()))?;
    }
    fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    let png_dir = out_dir.join("png");
    fs::create_dir_all(&png_dir).with_context(|| format!("creating {}", png_dir.display()))?;
    println!(
        "Scanning {} extractable MFT entries in {}",
        entry_count,
        gw_dat_path.display()
    );

    let mut manifest_entries = Vec::new();
    let mut failures = Vec::new();
    let mut recognized_payloads = 0_usize;
    let mut read_failures = 0_usize;
    let mut decode_failures = 0_usize;

    let mut scanned_entries = 0_usize;
    for entry_index in 0..archive.entries().len() {
        let entry = archive.entries()[entry_index];
        if entry.size == 0 || entry.content == 0 {
            continue;
        }
        scanned_entries += 1;
        let bytes = match archive.read_entry(entry.index) {
            Ok(bytes) => bytes,
            Err(error) => {
                read_failures += 1;
                failures.push(serde_json::json!({
                    "mft_entry_index": entry.index,
                    "stage": "read",
                    "error": format!("{error:#}"),
                }));
                continue;
            }
        };

        let payloads = if bytes.starts_with(b"ffna") {
            find_inline_atex_payloads(&bytes)
                .map(|(offset, bytes, header)| IconPayload::FfnaInlineAtex {
                    offset,
                    bytes,
                    header,
                })
                .collect::<Vec<_>>()
        } else {
            match decode_icon_payload(&bytes) {
                Ok(Some(payload)) => vec![payload],
                Ok(None) => continue,
                Err(error) => {
                    decode_failures += 1;
                    failures.push(serde_json::json!({
                        "mft_entry_index": entry.index,
                        "stage": "decode",
                        "magic_hex": hex::encode(&bytes[..bytes.len().min(8)]),
                        "error": format!("{error:#}"),
                    }));
                    continue;
                }
            }
        };

        let payload_count = payloads.len();
        recognized_payloads += payload_count;
        for (ordinal, payload) in payloads.into_iter().enumerate() {
            let filename = if payload_count == 1 {
                format!("{:06}.png", entry.index)
            } else {
                format!("{:06}-{ordinal}.png", entry.index)
            };
            let relative_path = format!("png/{filename}");
            let png_path = png_dir.join(&filename);

            if let Err(error) = payload.save_png(&png_path) {
                decode_failures += 1;
                failures.push(serde_json::json!({
                    "mft_entry_index": entry.index,
                    "payload_ordinal": ordinal,
                    "inline_texture_offset": payload.inline_texture_offset(),
                    "stage": "decode",
                    "kind": payload.kind(),
                    "format": payload.format(),
                    "width": payload.width(),
                    "height": payload.height(),
                    "error": format!("{error:#}"),
                }));
                continue;
            }

            manifest_entries.push(serde_json::json!({
                "mft_entry_index": entry.index,
                "payload_ordinal": ordinal,
                "file_ids": file_ids_by_mft.get(&entry.index).cloned().unwrap_or_default(),
                "content": entry.content,
                "content_type": entry.content_type,
                "compression": entry.compression,
                "kind": payload.kind(),
                "format": payload.format(),
                "width": payload.width(),
                "height": payload.height(),
                "inline_texture_offset": payload.inline_texture_offset(),
                "png": relative_path,
            }));
        }

        if scanned_entries.is_multiple_of(10_000) {
            println!(
                "Scanned {scanned_entries}/{entry_count} entries; exported {} images",
                manifest_entries.len()
            );
        }
    }

    let complete = read_failures == 0 && decode_failures == 0;
    let manifest = serde_json::json!({
        "schema_version": 1,
        "source": gw_dat_path,
        "complete": complete,
        "note": "Every decodable ATEX, ATTX, DDS, and inline FFNA ATEX/ATTX payload is exported as a web-ready PNG. One PNG is emitted per distinct MFT payload; file_ids lists every DAT hash alias whose linked MFT chain owns that payload.",
        "counts": {
            "scanned_mft_entries": entry_count,
            "recognized_image_payloads": recognized_payloads,
            "exported_pngs": manifest_entries.len(),
            "read_failures": read_failures,
            "decode_failures": decode_failures,
        },
        "entries": manifest_entries,
        "failures": failures,
    });
    write_json(&out_dir.join("manifest.json"), &manifest)?;
    println!(
        "Exported {} PNG images (read failures: {}, decode failures: {})",
        manifest["counts"]["exported_pngs"], read_failures, decode_failures
    );
    Ok(())
}

fn file_ids_by_mft_chain(archive: &DatArchive) -> BTreeMap<u32, BTreeSet<u32>> {
    let mut file_ids_by_mft = BTreeMap::<u32, BTreeSet<u32>>::new();
    for hash in archive.hash_lookup() {
        let mut mft_index = hash.mft_index;
        for _ in 0..256 {
            file_ids_by_mft
                .entry(mft_index)
                .or_default()
                .insert(hash.file_number);
            let Some(entry) = archive.entry(mft_index) else {
                break;
            };
            if entry.id == 0 || entry.id == mft_index {
                break;
            }
            mft_index = entry.id;
        }
    }
    file_ids_by_mft
}
