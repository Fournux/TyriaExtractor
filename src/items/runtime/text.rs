use super::catalog::*;
use super::*;

pub(super) const GENERIC_ITEM_NAME_TEXT_ID: u32 = 8326;

// Clean item names, plurals, brackets, etc.
pub(super) fn clean_item_name(text: &str) -> String {
    let mut text = text.replace("[lbracket]", "[").replace("[rbracket]", "]");

    // Replace bracket tags: [proper], [F], [M], [N], [PF], [PM], [U]
    let bracket_tags = ["proper", "F", "M", "N", "PF", "PM", "U"];
    for tag in &bracket_tags {
        text = text.replace(&format!("[{tag}]"), "");
    }

    // Remove plural/gender selector tags exactly like the Python validator:
    // [s], [pl:"..."], [f:"..."], and [m:"..."] are markup, not display text.
    while let Some(start) = text.find('[') {
        if let Some(end) = text[start..].find(']') {
            text.replace_range(start..=start + end, "");
        } else {
            break;
        }
    }

    // Strip HTML/XML tags
    while let Some(start) = text.find('<') {
        if let Some(end) = text[start..].find('>') {
            text.replace_range(start..=start + end, "");
        } else {
            break;
        }
    }

    // Collapse whitespace and trim
    let mut result = String::new();
    let mut in_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !in_space {
                result.push(' ');
                in_space = true;
            }
        } else {
            result.push(c);
            in_space = false;
        }
    }
    result
        .trim_matches(|c: char| c.is_whitespace() || c == '\0')
        .to_string()
}

pub(super) fn looks_like_item_name(raw_text: &str) -> bool {
    if raw_text.is_empty() || raw_text == "[null]" || raw_text == "..." || raw_text == "Unknown" {
        return false;
    }
    let lower_raw = raw_text.to_lowercase();
    let bad_substrings = [
        "%str",
        "%num",
        "guildwars.com",
        "http://",
        "https://",
        "<img",
        "\n",
    ];
    if bad_substrings.iter().any(|bad| lower_raw.contains(bad)) {
        return false;
    }

    let text = clean_item_name(raw_text);
    if text.is_empty() || text.len() < 2 || text.len() > 80 {
        return false;
    }
    if is_invalid_label(&text) {
        return false;
    }
    if !text.chars().any(|c| c.is_ascii_alphabetic()) {
        return false;
    }

    let has_digit = text.chars().any(|c| c.is_ascii_digit());
    if has_digit {
        let words: BTreeSet<&str> = text.split(|c: char| !c.is_alphanumeric()).collect();
        let allowed = ["1st", "2nd", "3rd", "Zaishen", "PvP"];
        if !allowed.iter().any(|&w| words.contains(w)) {
            return false;
        }
    }

    let lower = text.to_lowercase();
    let invalid_prefixes = [
        "you ",
        "your ",
        "target ",
        "for ",
        "while ",
        "if ",
        "when ",
        "this ",
        "use ",
        "speak",
        "stores",
        "applying",
        "enter",
        "choose",
        "double-click",
        "speak with",
        "stores ",
        "applying ",
        "enter ",
        "choose ",
    ];
    if invalid_prefixes
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return false;
    }
    if text.contains('?') || text.contains(';') || text.ends_with('.') {
        return false;
    }
    if text.ends_with('!') && text.split_whitespace().count() > 4 {
        return false;
    }
    if text.split_whitespace().count() > 7 {
        return false;
    }

    true
}

pub(super) fn is_invalid_label(name: &str) -> bool {
    let invalid_labels = [
        "Default",
        "Accept",
        "Reject",
        "Fail",
        "Fault",
        "Travel",
        "America",
        "Asia",
        "Europe",
        "International",
        "More Options...",
        "Active District",
        "Districts...",
        "Unknown",
        "Inconnu",
        "Unbekannt",
        "Desconocido",
        "Sconosciuto",
        "Nieznany",
        "未知的",
        "알 수 없음",
        "不明",
    ];
    invalid_labels.contains(&name)
}

#[cfg(test)]
pub(crate) fn encoded_values_for_test(hex: &str) -> Vec<u64> {
    encoded_values_from_hex(hex)
        .unwrap_or_default()
        .into_iter()
        .map(|(value, _, _)| value)
        .collect()
}

#[cfg(test)]
pub(crate) fn encoded_value_spans_for_test(hex: &str) -> (Vec<u16>, Vec<(u64, usize, usize)>) {
    let words = encoded_words_from_hex(hex).unwrap_or_default();
    let spans = encoded_values_from_words(&words).unwrap_or_default();
    (words, spans)
}

#[cfg(test)]
pub(crate) fn asyncdecode_item_ids_for_test(hex: &str) -> Vec<u32> {
    asyncdecode_item_ids_from_hex(hex).unwrap_or_default()
}

struct ItemTextSource {
    text_file_index: usize,
    ranges: &'static [(u32, u32)],
}

const ITEM_TEXT_SOURCES: &[ItemTextSource] = &[
    ItemTextSource {
        text_file_index: 2,
        ranges: &[(364, 399)], // common crafting materials
    },
    ItemTextSource {
        text_file_index: 8,
        ranges: &[(1, 33)], // early weapons and upgrades
    },
    ItemTextSource {
        text_file_index: 9,
        ranges: &[(0, 488)], // Prophecies weapons and armor
    },
    ItemTextSource {
        text_file_index: 10,
        ranges: &[(0, 119)], // trophies and collectibles
    },
    ItemTextSource {
        text_file_index: 20,
        ranges: &[(2, 127)], // Factions armor
    },
    ItemTextSource {
        text_file_index: 21,
        ranges: &[(0, 223)], // Factions armor, weapons, and upgrades
    },
    ItemTextSource {
        text_file_index: 27,
        ranges: &[(72, 86), (99, 375)], // unique weapons and Obsidian armor
    },
    ItemTextSource {
        text_file_index: 28,
        ranges: &[(0, 1023)], // Factions armor continuation
    },
    ItemTextSource {
        text_file_index: 29,
        ranges: &[(0, 811)], // Factions armor continuation
    },
    ItemTextSource {
        text_file_index: 31,
        ranges: &[(2, 10)], // starter weapons
    },
    ItemTextSource {
        text_file_index: 48,
        ranges: &[(0, 19)], // inscriptions and attribute scrolls
    },
    ItemTextSource {
        text_file_index: 56,
        ranges: &[(4, 4), (153, 154)], // holiday weapons
    },
    ItemTextSource {
        text_file_index: 63,
        ranges: &[(1, 2)], // Birthday Cupcake
    },
    ItemTextSource {
        text_file_index: 65,
        ranges: &[(0, 962)], // Eye of the North weapons and armor
    },
    ItemTextSource {
        text_file_index: 66,
        ranges: &[(0, 521)], // Eye of the North armor and miniatures
    },
    ItemTextSource {
        text_file_index: 67,
        ranges: &[(0, 0), (108, 122)], // Eye of the North quest and weapon tail
    },
    ItemTextSource {
        text_file_index: 70,
        ranges: &[(3, 4)], // dungeon maps and ale
    },
    ItemTextSource {
        text_file_index: 80,
        ranges: &[(0, 39)], // tournament tokens
    },
    ItemTextSource {
        text_file_index: 83,
        ranges: &[(1, 6)], // Zaishen coins
    },
    ItemTextSource {
        text_file_index: 85,
        ranges: &[(32, 36)], // store service items
    },
    ItemTextSource {
        text_file_index: 89,
        ranges: &[(0, 18)], // miniatures and White Mantle weapons
    },
    ItemTextSource {
        text_file_index: 94,
        ranges: &[(0, 28), (102, 118), (244, 244)], // Winds of Change
    },
];

pub(super) fn in_ranges(ordinal: u32, ranges: &[(u32, u32)]) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| ordinal >= start && ordinal <= end)
}

pub(super) fn calc_runtime_ordinal_base(file_id: u32) -> u32 {
    let offset = file_id.saturating_sub(185423);
    let shift = if offset <= 26 {
        0
    } else if offset <= 50 {
        1
    } else {
        2
    };
    offset.saturating_sub(shift) * TEXT_RECORDS_PER_FILE
}

pub(super) fn build_item_name_catalog(
    archive: &mut DatArchive,
    pe_data: &[u8],
    pe: &PeImage,
) -> Result<BTreeMap<u32, BTreeMap<String, String>>> {
    let decoded_records = BTreeMap::new();
    let compact_seeds = BTreeMap::new();
    let mut reader =
        LocalizedTextReader::new(archive, pe_data, pe, &compact_seeds, &decoded_records)?;
    let mut localized_names_by_id = BTreeMap::new();
    let mut seen = BTreeSet::new();

    for source in ITEM_TEXT_SOURCES {
        let Some(resource_file_id) = reader.file_id("en", source.text_file_index) else {
            continue;
        };
        let Some(entry_bytes) = reader.read_resource_file(resource_file_id)? else {
            continue;
        };
        let base_ordinal = calc_runtime_ordinal_base(resource_file_id);

        for record in text_records::parse_text_record_entries(&entry_bytes).with_context(|| {
            format!("parsing item text records from DAT file {resource_file_id}")
        })? {
            if !in_ranges(record.ordinal, source.ranges) {
                continue;
            }
            let raw_name = record
                .text
                .trim_end_matches('\0')
                .trim_start_matches('\u{feff}');
            if !looks_like_item_name(raw_name) {
                continue;
            }
            let name = clean_item_name(raw_name);
            if is_invalid_label(&name) {
                continue;
            }

            let string_id = base_ordinal + record.record_index;
            if !seen.insert((string_id, name.clone())) {
                continue;
            }

            let mut localized_name = reader
                .localized_record(source.text_file_index, record.record_index)?
                .into_iter()
                .filter_map(|(code, text)| {
                    let text = clean_item_name(&text);
                    (!text.is_empty()).then_some((code, text))
                })
                .collect::<BTreeMap<_, _>>();
            localized_name.insert("en".to_string(), name);
            localized_names_by_id.insert(string_id, localized_name);
        }
    }

    Ok(localized_names_by_id)
}

// ponytail: observed item AsyncDecode subset; add the full client opcode VM only when new captures break it.
pub(super) fn decode_encoded_name_fields(
    enc_name_hex: Option<&str>,
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    let hex = enc_name_hex?;
    asyncdecode_item_ids_from_hex(hex)
        .and_then(|ids| decode_name_fields_from_ids(&ids, by_text_id))
        .or_else(|| {
            let ids = encoded_name_ids_from_hex(hex)?;
            decode_name_fields_from_ids(&ids, by_text_id)
        })
}

pub(super) fn decode_encoded_description_fields(
    desc_enc_hex: Option<&str>,
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    let hex = desc_enc_hex?;
    asyncdecode_item_ids_from_hex(hex)
        .and_then(|ids| decode_text_fields_from_exact_ids(&ids, by_text_id, "description"))
        .or_else(|| {
            let ids = encoded_name_ids_from_hex(hex)?;
            decode_text_fields_from_exact_ids(&ids, by_text_id, "description")
        })
}

pub(super) fn decode_name_fields_from_exact_ids(
    ids: &[u32],
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    decode_text_fields_from_exact_ids(ids, by_text_id, "name")
}

pub(super) fn decode_description_fields_from_exact_ids(
    ids: &[u32],
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    decode_text_fields_from_exact_ids(ids, by_text_id, "description")
}

pub(super) fn decode_text_fields_from_exact_ids(
    ids: &[u32],
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
    prefix: &str,
) -> Option<BTreeMap<String, String>> {
    if let [id] = ids {
        return by_text_id
            .get(id)
            .map(|names| flat_runtime_text_fields(names, prefix))
            .filter(|fields| !fields.is_empty());
    }
    decode_text_fields_from_ids(ids, by_text_id, prefix)
}

pub(super) fn decode_name_fields_from_ids(
    ids: &[u32],
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    decode_text_fields_from_ids(ids, by_text_id, "name")
}

pub(super) fn decode_text_fields_from_ids(
    ids: &[u32],
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
    prefix: &str,
) -> Option<BTreeMap<String, String>> {
    let (template_id, arg_ids) = ids.split_first()?;
    if arg_ids.is_empty() {
        return None;
    }
    let template_names = by_text_id.get(template_id)?;
    let mut fields = BTreeMap::new();
    for (code, template) in template_names {
        let args = arg_ids
            .iter()
            .filter_map(|id| {
                by_text_id
                    .get(id)
                    .and_then(|names| names.get(code).or_else(|| names.get("en")))
                    .cloned()
            })
            .collect::<Vec<_>>();
        let text = clean_item_name(&apply_encoded_template(template, &args));
        if text.is_empty()
            || text == "[null]"
            || encoded_name_has_unresolved_placeholder(&text)
            || is_invalid_label(&text)
        {
            continue;
        }
        let text = clean_item_name(text.trim_end_matches('%'));
        if !text.is_empty() && !is_invalid_label(&text) {
            fields.insert(format!("{prefix}_{code}"), text);
        }
    }
    (!fields.is_empty()).then_some(fields)
}

const ASYNCDECODE_ITEM_CONTROL_WORDS: &[u16] = &[
    0x0a30, 0x0a31, 0x0a33, 0x0a34, 0x0a35, 0x0a3a, 0x0a3b, 0x0a3c, 0x0a3d, 0x0a3e, 0x0a3f, 0x0a40,
    0x0a42, 0x0a43, 0x0a7e, 0x0a80, 0x0a81, 0x0a84, 0x0a85, 0x0a86, 0x0a87, 0x0a88, 0x0a89, 0x0a8a,
    0x0a8b, 0x0aa4, 0x0aa7, 0x0aa8, 0x0aa9, 0x0aac, 0x0aaf, 0x0abb, 0x0abc,
];

const ASYNCDECODE_ITEM_CONTROL_IDS: &[u64] = &[37_404, 56_261, 69_415];

pub(super) fn encoded_name_ids_from_hex(hex: &str) -> Option<Vec<u32>> {
    Some(
        encoded_values_from_hex(hex)?
            .into_iter()
            .filter_map(|(value, _, _)| u32::try_from(value).ok())
            .collect(),
    )
}

pub(super) fn asyncdecode_item_ids_from_hex(hex: &str) -> Option<Vec<u32>> {
    let words = encoded_words_from_hex(hex)?;
    let values = encoded_values_from_words(&words)?;
    Some(
        values
            .into_iter()
            .filter(|(value, start, end)| {
                should_emit_asyncdecode_item_id(&words, *value, *start, *end)
            })
            .filter_map(|(value, _, _)| u32::try_from(value).ok())
            .collect(),
    )
}

pub(super) fn should_emit_asyncdecode_item_id(
    words: &[u16],
    value: u64,
    start: usize,
    end: usize,
) -> bool {
    if value == 2 {
        return start > 0
            && end < words.len()
            && words[start - 1] == 0x0002
            && words[end] == 0x0002;
    }
    if value > u64::from(u32::MAX) {
        return false;
    }
    if end == start + 1 {
        let word = words[start];
        return word >= 0x08d4 && !ASYNCDECODE_ITEM_CONTROL_WORDS.contains(&word);
    }
    !ASYNCDECODE_ITEM_CONTROL_IDS.contains(&value)
}

pub(super) fn encoded_name_seeds_from_hex(hex: &str) -> Option<BTreeMap<u32, u64>> {
    Some(
        text_references(&encoded_words_from_hex(hex)?)
            .into_iter()
            .filter(|reference| reference.seed > u64::from(u32::MAX))
            .map(|reference| (reference.id, reference.seed))
            .collect(),
    )
}

pub(super) fn encoded_name_has_unresolved_placeholder(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["%str", "%num", "%s", "%d"]
        .iter()
        .any(|placeholder| lower.contains(placeholder))
}

const ENCODED_TEXT_HEX_KEYS: &[&str] = &[
    "enc_name_hex",
    "complete_name_enc_hex",
    "desc_enc_hex",
    "description_enc_hex",
    "encoded_hex",
];

pub(super) fn insert_encoded_name_ids(name_ids: &mut BTreeSet<u32>, decoded: &serde_json::Value) {
    for key in ENCODED_TEXT_HEX_KEYS {
        if let Some(hex) = decoded.get(*key).and_then(|value| value.as_str())
            && let Some(ids) = encoded_name_ids_from_hex(hex)
        {
            name_ids.extend(ids);
        }
    }
}

pub(super) fn insert_encoded_name_seeds(
    seeds: &mut BTreeMap<u32, u64>,
    decoded: &serde_json::Value,
) {
    for key in ENCODED_TEXT_HEX_KEYS {
        if let Some(hex) = decoded.get(*key).and_then(|value| value.as_str())
            && let Some(parsed) = encoded_name_seeds_from_hex(hex)
        {
            seeds.extend(parsed);
        }
    }
}

pub(super) fn has_encoded_text_hex(decoded: &serde_json::Value) -> bool {
    ENCODED_TEXT_HEX_KEYS
        .iter()
        .any(|key| decoded.get(*key).is_some())
}

pub(super) fn insert_decoded_ids_by_encoded_hex(
    decoded_ids_by_encoded_hex: &mut BTreeMap<String, Vec<u32>>,
    decoded: &serde_json::Value,
) {
    let Some(ids) = value_u32_array(decoded, "decoded_ids") else {
        return;
    };
    for key in ENCODED_TEXT_HEX_KEYS {
        if let Some(hex) = decoded.get(*key).and_then(|value| value.as_str()) {
            decoded_ids_by_encoded_hex
                .entry(canonical_encoded_hex(hex))
                .or_insert_with(|| ids.clone());
        }
    }
}

pub(super) fn canonical_encoded_hex(hex: &str) -> String {
    let mut hex = hex.trim().to_ascii_lowercase();
    while hex.ends_with("0000") {
        hex.truncate(hex.len() - 4);
    }
    hex
}

pub(super) fn insert_text_decode_ids(name_ids: &mut BTreeSet<u32>, row: &serde_json::Value) {
    if row.get("kind").and_then(|kind| kind.as_str()) == Some("text_decode_ids") {
        if let Some(ids) = value_u32_array(row, "decoded_ids") {
            name_ids.extend(ids);
        }
        return;
    }
    let decoded = row.get("decoded").unwrap_or(row);
    if has_encoded_text_hex(decoded)
        && let Some(ids) = value_u32_array(decoded, "decoded_ids")
    {
        name_ids.extend(ids);
    }
}

#[derive(Debug, Default)]
pub(crate) struct PacketLogTextInputs {
    pub(crate) name_ids: BTreeSet<u32>,
    pub(crate) compact_seeds: BTreeMap<u32, u64>,
    pub(crate) decoded_records: BTreeMap<Vec<u8>, String>,
}

pub(crate) fn packet_log_text_inputs(
    packet_log_path: &Path,
    include_decoded_records: bool,
) -> Result<PacketLogTextInputs> {
    let mut inputs = PacketLogTextInputs::default();
    for_each_packet_log_row(packet_log_path, |row| {
        insert_text_decode_ids(&mut inputs.name_ids, &row);
        let decoded = row.get("decoded").unwrap_or(&row);
        if let Some(name_id) =
            value_u32(decoded, "name_text_id").or_else(|| value_u32(decoded, "name_id"))
        {
            inputs.name_ids.insert(name_id);
        }
        insert_encoded_name_ids(&mut inputs.name_ids, decoded);

        insert_encoded_name_seeds(&mut inputs.compact_seeds, &row);
        if let Some(decoded) = row.get("decoded") {
            insert_encoded_name_seeds(&mut inputs.compact_seeds, decoded);
        }

        if include_decoded_records {
            insert_decoded_text_record(&mut inputs.decoded_records, &row);
        }
    })?;
    Ok(inputs)
}

#[cfg(test)]
pub(crate) fn packet_log_name_ids(packet_log_path: &Path) -> Result<BTreeSet<u32>> {
    Ok(packet_log_text_inputs(packet_log_path, false)?.name_ids)
}

#[cfg(test)]
pub(crate) fn packet_log_name_seeds(packet_log_path: &Path) -> Result<BTreeMap<u32, u64>> {
    Ok(packet_log_text_inputs(packet_log_path, false)?.compact_seeds)
}

#[cfg(test)]
pub(crate) fn packet_log_decoded_text_records(
    packet_log_path: &Path,
) -> Result<BTreeMap<Vec<u8>, String>> {
    Ok(packet_log_text_inputs(packet_log_path, true)?.decoded_records)
}

pub(super) fn insert_decoded_text_record(
    records: &mut BTreeMap<Vec<u8>, String>,
    row: &serde_json::Value,
) {
    if row.get("kind").and_then(|kind| kind.as_str()) != Some("text_decode_trace") {
        return;
    }
    let (Some(record_hex), Some(text)) = (
        row.get("record_hex").and_then(|value| value.as_str()),
        row.get("output_preview").and_then(|value| value.as_str()),
    ) else {
        return;
    };
    let Some(record) = hex_to_bytes(record_hex) else {
        return;
    };
    match records.entry(record) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(text.to_string());
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            if trace_text_score(text) > trace_text_score(entry.get()) {
                entry.insert(text.to_string());
            }
        }
    }
}

pub(super) fn trace_text_score(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let printable = text.chars().filter(|ch| !ch.is_control()).count();
    if printable == 0 { 0 } else { printable + 1 }
}

pub(crate) fn runtime_item_text_lookup_with_compact_seeds(
    gw_dat_path: &Path,
    text_ids: &BTreeSet<u32>,
    compact_seeds: &BTreeMap<u32, u64>,
    decoded_records: &BTreeMap<Vec<u8>, String>,
) -> Result<RuntimeTextLookup> {
    let mut archive = DatArchive::open(gw_dat_path)?;
    let pe_data = archive.client_pe_data()?;
    let pe = PeImage::parse(&pe_data)?;
    let mut by_text_id = build_item_name_catalog(&mut archive, &pe_data, &pe)?;
    let by_model_file_id =
        scan_model_file_simple_name_links(&pe_data, pe.sections(), &by_text_id, &archive);
    let requested = resolve_localized_text_catalog_with_client(
        &mut archive,
        &pe_data,
        &pe,
        text_ids.iter().copied(),
        compact_seeds,
        decoded_records,
    )?;
    for (text_id, localized) in requested.by_text_id {
        by_text_id.entry(text_id).or_insert(localized);
    }
    Ok(RuntimeTextLookup {
        by_text_id,
        by_model_file_id,
    })
}
pub(super) fn scan_model_file_simple_name_links(
    pe_bytes: &[u8],
    pe_sections: &[PeSection],
    localized_names_by_id: &BTreeMap<u32, BTreeMap<String, String>>,
    archive: &DatArchive,
) -> BTreeMap<u32, BTreeMap<String, String>> {
    let Some(rdata) = pe_sections.iter().find(|section| section.name == ".rdata") else {
        return BTreeMap::new();
    };
    let raw_start = rdata.raw_pointer as usize;
    let raw_end = std::cmp::min(raw_start + rdata.raw_size as usize, pe_bytes.len());

    let mut candidate_starts = BTreeSet::new();
    let mut offset = raw_start;
    while offset + 8 <= raw_end {
        let model_file_id = u32::from_le_bytes([
            pe_bytes[offset],
            pe_bytes[offset + 1],
            pe_bytes[offset + 2],
            pe_bytes[offset + 3],
        ]);
        let name_string_id = u32::from_le_bytes([
            pe_bytes[offset + 4],
            pe_bytes[offset + 5],
            pe_bytes[offset + 6],
            pe_bytes[offset + 7],
        ]);

        if localized_names_by_id.contains_key(&name_string_id)
            && archive
                .mft_index_for_file_id(model_file_id)
                .is_some_and(|mft_index| archive.entry(mft_index).is_some())
        {
            candidate_starts.insert(offset);
        }
        offset += 4;
    }

    let run_len = |start: usize, stride: usize, candidate_starts: &BTreeSet<usize>| -> usize {
        let mut count = 0;
        let mut off = start;
        while candidate_starts.contains(&off) {
            count += 1;
            off += stride;
        }
        count
    };

    let mut covered = BTreeSet::new();
    let mut names_by_model_file_id =
        BTreeMap::<u32, BTreeMap<u32, BTreeMap<String, String>>>::new();

    for &start in &candidate_starts {
        if covered.contains(&start) {
            continue;
        }

        let len_24 = run_len(start, 0x24, &candidate_starts);
        let len_28 = run_len(start, 0x28, &candidate_starts);
        let (stride, count) = if len_28 >= len_24 {
            (0x28, len_28)
        } else {
            (0x24, len_24)
        };
        if count < 4 {
            continue;
        }

        for index in 0..count {
            let off = start + index * stride;
            covered.insert(off);
            let model_file_id = u32::from_le_bytes([
                pe_bytes[off],
                pe_bytes[off + 1],
                pe_bytes[off + 2],
                pe_bytes[off + 3],
            ]);
            let item_id = u32::from_le_bytes([
                pe_bytes[off + 4],
                pe_bytes[off + 5],
                pe_bytes[off + 6],
                pe_bytes[off + 7],
            ]);
            if let Some(names) = localized_names_by_id.get(&item_id) {
                names_by_model_file_id
                    .entry(model_file_id)
                    .or_default()
                    .insert(item_id, names.clone());
            }
        }
    }

    let mut out = BTreeMap::new();
    for (model_file_id, names_by_item_id) in names_by_model_file_id {
        let unique_names = names_by_item_id.values().cloned().collect::<BTreeSet<_>>();
        if unique_names.len() == 1
            && let Some(names) = unique_names.into_iter().next()
        {
            out.insert(model_file_id, names);
        }
    }
    out
}
