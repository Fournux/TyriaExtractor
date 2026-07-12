use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{BufRead, BufReader},
    path::Path,
};

use crate::{
    dat::{
        hash_lookup_by_file_id, lookup_mft_entry_for_file_id, lookup_mft_index_for_file_id,
        mft_entry_by_index, read_client_pe_data, read_dat_entry_from_file, read_dat_table,
        read_hash_lookup,
    },
    pe::{PeImage, PeSection},
    text::{
        apply_encoded_template, encoded_values_from_hex, encoded_values_from_words,
        encoded_words_from_hex, hex_to_bytes, text_references,
    },
    text_records::{
        self, CLIENT_LANGUAGE_CODES, CLIENT_TEXT_FILE_ID_TABLE_VA, CLIENT_TEXT_FILES_PER_LANGUAGE,
        TEXT_RECORDS_PER_FILE,
    },
};

const GENERIC_ITEM_NAME_TEXT_ID: u32 = 8326;

// Clean item names, plurals, brackets, etc.
fn clean_item_name(text: &str) -> String {
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

fn looks_like_item_name(raw_text: &str) -> bool {
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

fn is_invalid_label(name: &str) -> bool {
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

fn in_ranges(ordinal: u32, ranges: &[(u32, u32)]) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| ordinal >= start && ordinal <= end)
}

fn calc_runtime_ordinal_base(file_id: u32) -> u32 {
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

struct TextRecordLookupContext<'a> {
    metadata_len: u64,
    mft_entries: &'a [crate::models::MftEntry],
    hash_to_mft: &'a BTreeMap<u32, u32>,
    cache: &'a mut BTreeMap<u32, BTreeMap<u32, String>>,
    decoded_records: &'a BTreeMap<Vec<u8>, String>,
    compact_seeds: &'a BTreeMap<u32, u64>,
}

fn lookup_text_record(
    file: &mut File,
    context: &mut TextRecordLookupContext<'_>,
    file_ids: &[Option<u32>],
    file_index: usize,
    record_index: u32,
) -> Result<Option<String>> {
    let Some(file_id) = file_ids.get(file_index).and_then(|file_id| *file_id) else {
        return Ok(None);
    };
    if let std::collections::btree_map::Entry::Vacant(entry) = context.cache.entry(file_id) {
        let Some(mft_entry) =
            lookup_mft_entry_for_file_id(file_id, context.hash_to_mft, context.mft_entries)
        else {
            return Ok(None);
        };
        let entry_bytes = read_dat_entry_from_file(file, context.metadata_len, mft_entry)
            .with_context(|| {
                format!(
                    "reading text file {file_id} from MFT entry {}",
                    mft_entry.index
                )
            })?;
        let compact_seeds = context
            .compact_seeds
            .iter()
            .filter_map(|(&text_id, &seed)| {
                (text_id / TEXT_RECORDS_PER_FILE == file_index as u32)
                    .then_some((text_id % TEXT_RECORDS_PER_FILE, seed))
            })
            .collect::<BTreeMap<_, _>>();
        entry.insert(
            text_records::parse_text_record_map_with_decoded_records_and_seeds(
                &entry_bytes,
                context.decoded_records,
                &compact_seeds,
            )?,
        );
    }
    Ok(context
        .cache
        .get(&file_id)
        .and_then(|records| records.get(&record_index))
        .cloned())
}

fn build_item_name_catalog(
    file: &mut File,
    metadata_len: u64,
    mft_entries: &[crate::models::MftEntry],
    hash_to_mft: &BTreeMap<u32, u32>,
    pe_data: &[u8],
    pe: &PeImage,
) -> Result<BTreeMap<u32, BTreeMap<String, String>>> {
    // English drives row selection; output names include every available client language.
    let file_ids = pe.language_file_ids(
        pe_data,
        CLIENT_TEXT_FILE_ID_TABLE_VA,
        CLIENT_TEXT_FILES_PER_LANGUAGE,
        0,
    )?;
    let localized_file_ids = CLIENT_LANGUAGE_CODES
        .iter()
        .map(|(language_index, code)| {
            pe.language_file_ids(
                pe_data,
                CLIENT_TEXT_FILE_ID_TABLE_VA,
                CLIENT_TEXT_FILES_PER_LANGUAGE,
                *language_index,
            )
            .map(|ids| (*code, ids))
        })
        .collect::<Result<Vec<_>>>()?;

    let mut localized_names_by_id = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut text_records_cache = BTreeMap::<u32, BTreeMap<u32, String>>::new();
    let decoded_records = BTreeMap::new();
    let compact_seeds = BTreeMap::new();
    let mut text_lookup_context = TextRecordLookupContext {
        metadata_len,
        mft_entries,
        hash_to_mft,
        cache: &mut text_records_cache,
        decoded_records: &decoded_records,
        compact_seeds: &compact_seeds,
    };

    for source in ITEM_TEXT_SOURCES {
        let Some(Some(resource_file_id)) = file_ids.get(source.text_file_index) else {
            continue;
        };
        let Some(mft_entry) =
            lookup_mft_entry_for_file_id(*resource_file_id, hash_to_mft, mft_entries)
        else {
            continue;
        };
        let entry_bytes = read_dat_entry_from_file(file, metadata_len, mft_entry)?;
        let base_ordinal = calc_runtime_ordinal_base(*resource_file_id);

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

            let mut localized_name = BTreeMap::new();
            for (code, language_file_ids) in &localized_file_ids {
                let text = if *code == "en" {
                    Some(name.clone())
                } else {
                    lookup_text_record(
                        file,
                        &mut text_lookup_context,
                        language_file_ids,
                        source.text_file_index,
                        record.record_index,
                    )?
                    .map(|text| clean_item_name(&text))
                };
                if let Some(text) = text.filter(|text| !text.is_empty()) {
                    localized_name.insert((*code).to_string(), text);
                }
            }
            localized_names_by_id.insert(string_id, localized_name);
        }
    }

    Ok(localized_names_by_id)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RuntimeItemObservation {
    model_id: u32,
    model_file_id: u32,
    item_type: Option<u32>,
    extra_id: Option<u32>,
    materials: Option<u32>,
    interaction: Option<u32>,
    price: Option<u32>,
    name_id: Option<u32>,
    name_id_is_exact: bool,
    enc_name_hex: Option<String>,
    desc_enc_hex: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct RuntimeTextLookup {
    pub(crate) by_text_id: BTreeMap<u32, BTreeMap<String, String>>,
    pub(crate) by_model_file_id: BTreeMap<u32, BTreeMap<String, String>>,
}

impl RuntimeTextLookup {
    fn name_fields_for(&self, observation: &RuntimeItemObservation) -> BTreeMap<String, String> {
        if let Some(names) =
            decode_encoded_name_fields(observation.enc_name_hex.as_deref(), &self.by_text_id)
            && !names.is_empty()
        {
            return self.with_model_file_names(observation, names);
        }
        if observation.name_id_is_exact
            && let Some(name_id) = observation.name_id
            && let Some(names) = self.by_text_id.get(&name_id).map(flat_runtime_name_fields)
            && !names.is_empty()
        {
            return self.with_model_file_names(observation, names);
        }

        if observation.enc_name_hex.is_some() {
            let names = self.model_file_name_fields_for(observation);
            return if names.is_empty() {
                self.generic_name_fields_for(observation)
            } else {
                names
            };
        }

        let names = self.fallback_name_fields_for(observation);
        if names.is_empty() {
            self.generic_name_fields_for(observation)
        } else {
            names
        }
    }

    fn model_file_name_fields_for(
        &self,
        observation: &RuntimeItemObservation,
    ) -> BTreeMap<String, String> {
        self.by_model_file_id
            .get(&observation.model_file_id)
            .map(flat_runtime_name_fields)
            .unwrap_or_default()
    }

    fn with_model_file_names(
        &self,
        observation: &RuntimeItemObservation,
        mut names: BTreeMap<String, String>,
    ) -> BTreeMap<String, String> {
        for (key, value) in self.model_file_name_fields_for(observation) {
            names.entry(key).or_insert(value);
        }
        names
    }

    fn fallback_name_fields_for(
        &self,
        observation: &RuntimeItemObservation,
    ) -> BTreeMap<String, String> {
        let names = observation
            .name_id
            .and_then(|name_id| self.by_text_id.get(&name_id))
            .map(flat_runtime_name_fields)
            .unwrap_or_default();
        self.with_model_file_names(observation, names)
    }

    fn generic_name_fields_for(
        &self,
        observation: &RuntimeItemObservation,
    ) -> BTreeMap<String, String> {
        if observation.name_id != Some(GENERIC_ITEM_NAME_TEXT_ID) {
            return BTreeMap::new();
        }
        self.by_text_id
            .get(&GENERIC_ITEM_NAME_TEXT_ID)
            .map(|names| {
                names
                    .iter()
                    .map(|(code, name)| (format!("name_{code}"), name.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn validated_name_fields_for(
        &self,
        observation: &RuntimeItemObservation,
        official_name: &str,
    ) -> Option<BTreeMap<String, String>> {
        if let Some(ids) = observation
            .enc_name_hex
            .as_deref()
            .and_then(encoded_name_ids_from_hex)
            && let Some(names) = decode_name_fields_from_exact_ids(&ids, &self.by_text_id)
            && multilingual_names_match(&names, official_name)
        {
            return Some(self.with_model_file_names(observation, names));
        }
        if let Some(names) =
            decode_encoded_name_fields(observation.enc_name_hex.as_deref(), &self.by_text_id)
            && multilingual_names_match(&names, official_name)
        {
            return Some(self.with_model_file_names(observation, names));
        }

        let names =
            self.with_model_file_names(observation, self.fallback_name_fields_for(observation));
        multilingual_names_match(&names, official_name).then_some(names)
    }
}

#[derive(Debug, Serialize)]
struct RuntimeDetectedItem {
    model_id: u32,
    model_file_id: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    item_ids: Vec<u32>,
    #[serde(rename = "packet_name_id")]
    #[serde(skip_serializing_if = "Option::is_none")]
    name_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    item_type: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extra_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    materials: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    interaction: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    price: Option<u32>,
    #[serde(flatten)]
    strings: BTreeMap<String, String>,
}

type RuntimeItemKey = (u32, Option<u32>, Option<u32>);

fn for_each_packet_log_row(
    packet_log_path: &Path,
    mut visit: impl FnMut(serde_json::Value),
) -> Result<()> {
    let file = File::open(packet_log_path)
        .with_context(|| format!("opening packet log {}", packet_log_path.display()))?;
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| {
            format!(
                "reading packet log {} line {}",
                packet_log_path.display(),
                line_index + 1
            )
        })?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let row = serde_json::from_str(line).with_context(|| {
            format!(
                "parsing packet log {} line {}",
                packet_log_path.display(),
                line_index + 1
            )
        })?;
        visit(row);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn export_detected_items_from_packet_log(
    packet_log_path: &Path,
    name_lookup: &RuntimeTextLookup,
    out_path: &Path,
) -> Result<()> {
    export_detected_items_from_packet_log_with_client_strings(
        packet_log_path,
        name_lookup,
        out_path,
        true,
    )
}

pub(crate) fn export_detected_items_from_packet_log_with_client_strings(
    packet_log_path: &Path,
    name_lookup: &RuntimeTextLookup,
    out_path: &Path,
    use_client_strings: bool,
) -> Result<()> {
    let mut decoded_item_rows = 0_usize;
    let mut observations = BTreeMap::<RuntimeItemObservation, BTreeSet<u32>>::new();
    let mut client_names_by_item = BTreeMap::<RuntimeItemKey, BTreeMap<String, String>>::new();
    let mut client_descriptions_by_item =
        BTreeMap::<RuntimeItemKey, BTreeMap<String, String>>::new();
    let mut client_name_rows = 0_usize;
    let mut client_description_rows = 0_usize;
    let mut runtime_string_rows = 0_usize;
    let mut decoded_ids_by_encoded_hex = BTreeMap::<String, Vec<u32>>::new();
    let mut runtime_desc_hex_by_item = BTreeMap::<RuntimeItemKey, String>::new();
    let mut runtime_name_hex_by_item = BTreeMap::<RuntimeItemKey, String>::new();

    for_each_packet_log_row(packet_log_path, |row| {
        match row.get("kind").and_then(|kind| kind.as_str()) {
            Some("text_decode_ids") => {
                if let (Some(hex), Some(ids)) = (
                    row.get("encoded_hex").and_then(|value| value.as_str()),
                    value_u32_array(&row, "decoded_ids"),
                ) {
                    decoded_ids_by_encoded_hex
                        .entry(canonical_encoded_hex(hex))
                        .or_insert(ids);
                }
                return;
            }
            Some("decoded_name") => {
                if use_client_strings
                    && insert_client_item_string(&row, "name", "name", &mut client_names_by_item)
                {
                    client_name_rows += 1;
                }
                return;
            }
            Some("decoded_description") => {
                if use_client_strings
                    && insert_client_item_string(
                        &row,
                        "description",
                        "description",
                        &mut client_descriptions_by_item,
                    )
                {
                    client_description_rows += 1;
                }
                return;
            }
            Some("runtime_item_strings") => {
                runtime_string_rows += 1;
                insert_runtime_item_hex(
                    &row,
                    &["desc_enc_hex", "description_enc_hex"],
                    &mut runtime_desc_hex_by_item,
                );
                insert_runtime_item_hex(
                    &row,
                    &["complete_name_enc_hex"],
                    &mut runtime_name_hex_by_item,
                );
                insert_decoded_ids_by_encoded_hex(&mut decoded_ids_by_encoded_hex, &row);
                return;
            }
            _ => {}
        }

        let decoded = row.get("decoded").unwrap_or(&row);
        if use_client_strings {
            if insert_client_item_string(decoded, "decoded_name", "name", &mut client_names_by_item)
            {
                client_name_rows += 1;
            }
            if insert_client_item_string(
                decoded,
                "decoded_description",
                "description",
                &mut client_descriptions_by_item,
            ) {
                client_description_rows += 1;
            }
        }
        insert_decoded_ids_by_encoded_hex(&mut decoded_ids_by_encoded_hex, decoded);
        let Some(observation) = runtime_item_observation(decoded) else {
            return;
        };
        decoded_item_rows += 1;
        observations
            .entry(observation)
            .or_default()
            .extend(value_u32(decoded, "item_id"));
    })?;

    let mut items_by_model = BTreeMap::<(u32, u32), RuntimeDetectedItem>::new();
    for (observation, item_ids) in observations {
        let item = RuntimeDetectedItem {
            model_id: observation.model_id,
            model_file_id: observation.model_file_id,
            item_ids: item_ids.iter().copied().collect(),
            name_id: observation.name_id,
            item_type: observation.item_type,
            extra_id: observation.extra_id,
            materials: observation.materials,
            interaction: observation.interaction,
            price: observation.price,
            strings: {
                let runtime_name_hex = item_ids.iter().find_map(|item_id| {
                    runtime_name_hex_by_item
                        .get(&(
                            *item_id,
                            Some(observation.model_id),
                            Some(observation.model_file_id),
                        ))
                        .or_else(|| runtime_name_hex_by_item.get(&(*item_id, None, None)))
                });
                let local_names = name_lookup.with_model_file_names(
                    &observation,
                    observation
                        .enc_name_hex
                        .as_ref()
                        .and_then(|hex| decoded_ids_by_encoded_hex.get(&canonical_encoded_hex(hex)))
                        .and_then(|ids| {
                            decode_name_fields_from_exact_ids(ids, &name_lookup.by_text_id)
                        })
                        .or_else(|| {
                            runtime_name_hex
                                .and_then(|hex| {
                                    decoded_ids_by_encoded_hex.get(&canonical_encoded_hex(hex))
                                })
                                .and_then(|ids| {
                                    decode_name_fields_from_exact_ids(ids, &name_lookup.by_text_id)
                                })
                        })
                        .or_else(|| {
                            decode_encoded_name_fields(
                                runtime_name_hex.map(String::as_str),
                                &name_lookup.by_text_id,
                            )
                        })
                        .unwrap_or_else(|| name_lookup.name_fields_for(&observation)),
                );
                let mut strings = item_ids
                    .iter()
                    .find_map(|item_id| {
                        client_names_by_item
                            .get(&(
                                *item_id,
                                Some(observation.model_id),
                                Some(observation.model_file_id),
                            ))
                            .or_else(|| client_names_by_item.get(&(*item_id, None, None)))
                            .map(|client_names| {
                                client_runtime_name_fields(client_names, &observation, name_lookup)
                            })
                            .filter(|names| !names.is_empty())
                    })
                    .unwrap_or(local_names);
                let desc_hex = item_ids
                    .iter()
                    .find_map(|item_id| {
                        runtime_desc_hex_by_item
                            .get(&(
                                *item_id,
                                Some(observation.model_id),
                                Some(observation.model_file_id),
                            ))
                            .or_else(|| runtime_desc_hex_by_item.get(&(*item_id, None, None)))
                    })
                    .or(observation.desc_enc_hex.as_ref());
                let local_descriptions = desc_hex
                    .and_then(|hex| decoded_ids_by_encoded_hex.get(&canonical_encoded_hex(hex)))
                    .and_then(|ids| {
                        decode_description_fields_from_exact_ids(ids, &name_lookup.by_text_id)
                    })
                    .or_else(|| {
                        decode_encoded_description_fields(
                            desc_hex.map(String::as_str),
                            &name_lookup.by_text_id,
                        )
                    })
                    .unwrap_or_default();
                let descriptions = item_ids
                    .iter()
                    .find_map(|item_id| {
                        client_descriptions_by_item
                            .get(&(
                                *item_id,
                                Some(observation.model_id),
                                Some(observation.model_file_id),
                            ))
                            .or_else(|| client_descriptions_by_item.get(&(*item_id, None, None)))
                            .map(client_runtime_description_fields)
                            .filter(|descriptions| !descriptions.is_empty())
                    })
                    .unwrap_or(local_descriptions);
                strings.extend(descriptions);
                strings
            },
        };

        match items_by_model.entry((item.model_id, item.model_file_id)) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(item);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if item.strings.len() > entry.get().strings.len() {
                    entry.insert(item);
                }
            }
        }
    }

    let items = items_by_model.into_values().collect::<Vec<_>>();

    let named_items = items
        .iter()
        .filter(|item| has_name_field(&item.strings))
        .count();
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let out_file =
        File::create(out_path).with_context(|| format!("creating {}", out_path.display()))?;
    serde_json::to_writer_pretty(out_file, &items)?;
    println!(
        "wrote {} runtime item rows from {} decoded packet rows, {} client decoded name rows, {} client decoded description rows, {} runtime string rows, and {} named item rows to {}",
        items.len(),
        decoded_item_rows,
        client_name_rows,
        client_description_rows,
        runtime_string_rows,
        named_items,
        out_path.display()
    );
    Ok(())
}

fn runtime_item_observation(decoded: &serde_json::Value) -> Option<RuntimeItemObservation> {
    let model_id = value_u32(decoded, "model_id")?;
    let model_file_id = value_model_file_id(decoded)?;
    let name_text_id = value_u32(decoded, "name_text_id");
    Some(RuntimeItemObservation {
        model_id,
        model_file_id,
        item_type: value_u32(decoded, "item_type").or_else(|| value_u32(decoded, "type")),
        extra_id: value_u32(decoded, "extra_id"),
        materials: value_u32(decoded, "materials"),
        interaction: value_u32(decoded, "interaction"),
        price: value_u32(decoded, "price"),
        name_id: name_text_id.or_else(|| value_u32(decoded, "name_id")),
        name_id_is_exact: name_text_id.is_some(),
        enc_name_hex: decoded
            .get("enc_name_hex")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        desc_enc_hex: decoded
            .get("desc_enc_hex")
            .or_else(|| decoded.get("description_enc_hex"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
    })
}

fn value_u32(value: &serde_json::Value, key: &str) -> Option<u32> {
    value
        .get(key)?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
}

fn value_model_file_id(value: &serde_json::Value) -> Option<u32> {
    value_u32(value, "model_file_id").map(|value| value & 0x7fff_ffff)
}

fn runtime_item_key(row: &serde_json::Value) -> Option<RuntimeItemKey> {
    Some((
        value_u32(row, "item_id")?,
        value_u32(row, "model_id"),
        value_model_file_id(row),
    ))
}

fn value_u32_array(value: &serde_json::Value, key: &str) -> Option<Vec<u32>> {
    value
        .get(key)?
        .as_array()?
        .iter()
        .map(|item| item.as_u64().and_then(|value| u32::try_from(value).ok()))
        .collect()
}

fn insert_runtime_item_hex(
    row: &serde_json::Value,
    keys: &[&str],
    strings_by_item: &mut BTreeMap<RuntimeItemKey, String>,
) {
    let Some(key) = runtime_item_key(row) else {
        return;
    };
    let Some(hex) = keys
        .iter()
        .find_map(|key| row.get(*key).and_then(|value| value.as_str()))
        .filter(|hex| !hex.is_empty())
    else {
        return;
    };
    strings_by_item
        .entry(key)
        .or_insert_with(|| hex.to_string());
}

fn insert_client_item_string(
    row: &serde_json::Value,
    source_key: &str,
    default_code: &str,
    strings_by_item: &mut BTreeMap<RuntimeItemKey, BTreeMap<String, String>>,
) -> bool {
    let Some(key) = runtime_item_key(row) else {
        return false;
    };
    let Some(text) = row
        .get(source_key)
        .and_then(|value| value.as_str())
        .map(clean_item_name)
        .filter(|text| !text.is_empty() && !is_invalid_label(text))
    else {
        return false;
    };
    let code = row
        .get("lang")
        .and_then(|value| value.as_str())
        .unwrap_or(default_code);
    strings_by_item
        .entry(key)
        .or_default()
        .insert(code.to_string(), text);
    true
}

fn client_runtime_name_fields(
    names: &BTreeMap<String, String>,
    observation: &RuntimeItemObservation,
    name_lookup: &RuntimeTextLookup,
) -> BTreeMap<String, String> {
    let unique = valid_unique_texts(names);
    if unique.len() == 1 {
        let Some(name) = unique.into_iter().next() else {
            return BTreeMap::new();
        };
        if let Some(local_names) = name_lookup.validated_name_fields_for(observation, name) {
            return local_names;
        }
        return BTreeMap::from([("name".to_string(), name.clone())]);
    }
    flat_runtime_name_fields(names)
}

fn client_runtime_description_fields(
    descriptions: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let unique = valid_unique_texts(descriptions);
    if unique.len() == 1 {
        let Some(description) = unique.into_iter().next() else {
            return BTreeMap::new();
        };
        return BTreeMap::from([("description".to_string(), description.clone())]);
    }
    flat_runtime_description_fields(descriptions)
}

fn valid_unique_texts(strings: &BTreeMap<String, String>) -> BTreeSet<&String> {
    strings
        .values()
        .filter(|text| !is_invalid_label(text))
        .collect()
}

fn has_name_field(strings: &BTreeMap<String, String>) -> bool {
    strings
        .keys()
        .any(|key| key == "name" || key.starts_with("name_"))
}

fn multilingual_names_match(names: &BTreeMap<String, String>, official_name: &str) -> bool {
    localized_name_score(names) > 1
        && names
            .values()
            .any(|name| !is_invalid_label(name) && name == official_name)
}

fn localized_name_score(names: &BTreeMap<String, String>) -> usize {
    valid_unique_texts(names).len()
}

fn flat_runtime_name_fields(names: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    names
        .iter()
        .filter_map(|(code, text)| {
            let text = clean_item_name(text);
            (!text.is_empty() && !is_invalid_label(&text)).then(|| (format!("name_{code}"), text))
        })
        .collect()
}

fn flat_runtime_description_fields(
    descriptions: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    flat_runtime_text_fields(descriptions, "description")
}

fn flat_runtime_text_fields(
    strings: &BTreeMap<String, String>,
    prefix: &str,
) -> BTreeMap<String, String> {
    strings
        .iter()
        .filter(|(_, text)| !is_invalid_label(text))
        .map(|(code, text)| (format!("{prefix}_{code}"), text.clone()))
        .collect()
}

// ponytail: observed item AsyncDecode subset; add the full client opcode VM only when new captures break it.
fn decode_encoded_name_fields(
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

fn decode_encoded_description_fields(
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

fn decode_name_fields_from_exact_ids(
    ids: &[u32],
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    decode_text_fields_from_exact_ids(ids, by_text_id, "name")
}

fn decode_description_fields_from_exact_ids(
    ids: &[u32],
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    decode_text_fields_from_exact_ids(ids, by_text_id, "description")
}

fn decode_text_fields_from_exact_ids(
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

fn decode_name_fields_from_ids(
    ids: &[u32],
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    decode_text_fields_from_ids(ids, by_text_id, "name")
}

fn decode_text_fields_from_ids(
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

fn encoded_name_ids_from_hex(hex: &str) -> Option<Vec<u32>> {
    Some(
        encoded_values_from_hex(hex)?
            .into_iter()
            .filter_map(|(value, _, _)| u32::try_from(value).ok())
            .collect(),
    )
}

fn asyncdecode_item_ids_from_hex(hex: &str) -> Option<Vec<u32>> {
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

fn should_emit_asyncdecode_item_id(words: &[u16], value: u64, start: usize, end: usize) -> bool {
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

fn encoded_name_seeds_from_hex(hex: &str) -> Option<BTreeMap<u32, u64>> {
    Some(
        text_references(&encoded_words_from_hex(hex)?)
            .into_iter()
            .filter(|reference| reference.seed > u64::from(u32::MAX))
            .map(|reference| (reference.id, reference.seed))
            .collect(),
    )
}

fn encoded_name_has_unresolved_placeholder(name: &str) -> bool {
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

fn insert_encoded_name_ids(name_ids: &mut BTreeSet<u32>, decoded: &serde_json::Value) {
    for key in ENCODED_TEXT_HEX_KEYS {
        if let Some(hex) = decoded.get(*key).and_then(|value| value.as_str())
            && let Some(ids) = encoded_name_ids_from_hex(hex)
        {
            name_ids.extend(ids);
        }
    }
}

fn insert_encoded_name_seeds(seeds: &mut BTreeMap<u32, u64>, decoded: &serde_json::Value) {
    for key in ENCODED_TEXT_HEX_KEYS {
        if let Some(hex) = decoded.get(*key).and_then(|value| value.as_str())
            && let Some(parsed) = encoded_name_seeds_from_hex(hex)
        {
            seeds.extend(parsed);
        }
    }
}

fn has_encoded_text_hex(decoded: &serde_json::Value) -> bool {
    ENCODED_TEXT_HEX_KEYS
        .iter()
        .any(|key| decoded.get(*key).is_some())
}

fn insert_decoded_ids_by_encoded_hex(
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

fn canonical_encoded_hex(hex: &str) -> String {
    let mut hex = hex.trim().to_ascii_lowercase();
    while hex.ends_with("0000") {
        hex.truncate(hex.len() - 4);
    }
    hex
}

fn insert_text_decode_ids(name_ids: &mut BTreeSet<u32>, row: &serde_json::Value) {
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

fn insert_decoded_text_record(records: &mut BTreeMap<Vec<u8>, String>, row: &serde_json::Value) {
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

fn trace_text_score(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let printable = text.chars().filter(|ch| !ch.is_control()).count();
    if printable == 0 { 0 } else { printable + 1 }
}

pub(crate) fn runtime_text_lookup_with_compact_seeds(
    gw_dat_path: &Path,
    text_ids: &BTreeSet<u32>,
    compact_seeds: &BTreeMap<u32, u64>,
    decoded_records: &BTreeMap<Vec<u8>, String>,
) -> Result<RuntimeTextLookup> {
    runtime_text_lookup(gw_dat_path, text_ids, compact_seeds, decoded_records, false)
}

pub(crate) fn runtime_item_text_lookup_with_compact_seeds(
    gw_dat_path: &Path,
    text_ids: &BTreeSet<u32>,
    compact_seeds: &BTreeMap<u32, u64>,
    decoded_records: &BTreeMap<Vec<u8>, String>,
) -> Result<RuntimeTextLookup> {
    runtime_text_lookup(gw_dat_path, text_ids, compact_seeds, decoded_records, true)
}

fn runtime_text_lookup(
    gw_dat_path: &Path,
    text_ids: &BTreeSet<u32>,
    compact_seeds: &BTreeMap<u32, u64>,
    decoded_records: &BTreeMap<Vec<u8>, String>,
    include_item_catalog: bool,
) -> Result<RuntimeTextLookup> {
    if text_ids.is_empty() && !include_item_catalog {
        return Ok(RuntimeTextLookup::default());
    }
    let metadata = fs::metadata(gw_dat_path)
        .with_context(|| format!("reading metadata for {}", gw_dat_path.display()))?;
    let mut file =
        File::open(gw_dat_path).with_context(|| format!("opening {}", gw_dat_path.display()))?;
    let (_, _, mft_entries) = read_dat_table(&mut file, gw_dat_path, metadata.len())?;
    let hash_lookup = read_hash_lookup(&mut file, metadata.len(), &mft_entries)?;
    let hash_to_mft = hash_lookup_by_file_id(&hash_lookup);
    let pe_data = read_client_pe_data(
        gw_dat_path,
        &mut file,
        metadata.len(),
        &hash_to_mft,
        &mft_entries,
    )?;
    let pe = PeImage::parse(&pe_data)?;
    let mut by_text_id = if include_item_catalog {
        build_item_name_catalog(
            &mut file,
            metadata.len(),
            &mft_entries,
            &hash_to_mft,
            &pe_data,
            &pe,
        )?
    } else {
        BTreeMap::new()
    };
    let by_model_file_id = if include_item_catalog {
        scan_model_file_simple_name_links(
            &pe_data,
            pe.sections(),
            &by_text_id,
            &hash_to_mft,
            &mft_entries,
        )
    } else {
        BTreeMap::new()
    };
    let mut text_records_cache = BTreeMap::<u32, BTreeMap<u32, String>>::new();
    let mut context = TextRecordLookupContext {
        metadata_len: metadata.len(),
        mft_entries: &mft_entries,
        hash_to_mft: &hash_to_mft,
        cache: &mut text_records_cache,
        compact_seeds,
        decoded_records,
    };
    add_requested_runtime_texts(
        &mut file,
        &pe_data,
        &pe,
        text_ids,
        &mut by_text_id,
        &mut context,
    )?;
    Ok(RuntimeTextLookup {
        by_text_id,
        by_model_file_id,
    })
}
fn add_requested_runtime_texts(
    file: &mut File,
    pe_data: &[u8],
    pe: &PeImage,
    text_ids: &BTreeSet<u32>,
    names_by_text_id: &mut BTreeMap<u32, BTreeMap<String, String>>,
    context: &mut TextRecordLookupContext<'_>,
) -> Result<()> {
    if text_ids.is_empty() {
        return Ok(());
    }

    let localized_file_ids = CLIENT_LANGUAGE_CODES
        .iter()
        .map(|(language_index, code)| {
            pe.language_file_ids(
                pe_data,
                CLIENT_TEXT_FILE_ID_TABLE_VA,
                CLIENT_TEXT_FILES_PER_LANGUAGE,
                *language_index,
            )
            .map(|ids| (*code, ids))
        })
        .collect::<Result<Vec<_>>>()?;

    for &text_id in text_ids {
        if names_by_text_id.contains_key(&text_id) {
            continue;
        }
        let file_index = (text_id / TEXT_RECORDS_PER_FILE) as usize;
        let record_index = text_id % TEXT_RECORDS_PER_FILE;
        let mut names = BTreeMap::new();
        for (code, file_ids) in &localized_file_ids {
            let Some(text) = lookup_text_record(file, context, file_ids, file_index, record_index)?
                .filter(|text| !text.is_empty())
            else {
                continue;
            };
            names.insert((*code).to_string(), text);
        }
        if !names.is_empty() {
            names_by_text_id.insert(text_id, names);
        }
    }

    Ok(())
}
fn scan_model_file_simple_name_links(
    pe_bytes: &[u8],
    pe_sections: &[PeSection],
    localized_names_by_id: &BTreeMap<u32, BTreeMap<String, String>>,
    hash_to_mft: &BTreeMap<u32, u32>,
    mft_entries: &[crate::models::MftEntry],
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
            && lookup_mft_index_for_file_id(model_file_id, hash_to_mft)
                .is_some_and(|mft_index| mft_entry_by_index(mft_entries, mft_index).is_some())
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
