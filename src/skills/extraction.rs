use super::{
    ExtractedSkill, OutputCampaignStats, OutputCounts, OutputManifest, SKILL_FLAG_NOT_PLAYABLE,
    SKILL_FLAG_PVE, SKILL_FLAG_PVP, SkillCosts, SkillFlags, SkillTiming, campaign_name,
    decoded_energy_cost,
    icons::export_skill_icon,
    profession_name, skill_type_name,
    table::{SKILL_RECORD_SIZE, locate_skill_table},
};

use anyhow::Context;
use chrono::Utc;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    path::Path,
};

use crate::{
    dat::{
        hash_lookup_by_file_id, lookup_mft_index_for_file_id, mft_entry_by_index,
        read_client_pe_data, read_dat_entry_from_file, read_dat_table, read_hash_lookup,
    },
    pe::PeImage,
    text_records::{
        self, CLIENT_LANGUAGE_CODES, CLIENT_TEXT_FILE_ID_TABLE_VA, CLIENT_TEXT_FILES_PER_LANGUAGE,
        TEXT_RECORDS_PER_FILE,
    },
};

fn clean_text(text: &str) -> String {
    let mut text = text.replace("[lbracket]", "[").replace("[rbracket]", "]");
    for tag in ["[proper]", "[F]", "[M]", "[N]", "[PF]", "[PM]", "[U]"] {
        text = text.replace(tag, "");
    }
    while let Some(start) = text.find('<') {
        if let Some(end) = text[start..].find('>') {
            text.replace_range(start..=start + end, "");
        } else {
            break;
        }
    }
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

pub(crate) fn extract_skills_to_model_file_dirs(
    gw_dat_path: &Path,
    out_path: &Path,
    model_file_dir: &Path,
    model_file_hd_dir: &Path,
) -> anyhow::Result<()> {
    extract_skills_with_icon_dirs(
        gw_dat_path,
        out_path,
        model_file_dir,
        Some(model_file_hd_dir),
    )
}

fn extract_skills_with_icon_dirs(
    gw_dat_path: &Path,
    out_path: &Path,
    images_dir: &Path,
    images_hd_dir: Option<&Path>,
) -> anyhow::Result<()> {
    let metadata =
        fs::metadata(gw_dat_path).with_context(|| format!("reading {}", gw_dat_path.display()))?;
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

    // Extract skill metadata records from the PE skill table
    let skill_table = locate_skill_table(&pe_data, &pe)?;
    let skill_table_offset = skill_table.file_offset;
    let skill_table_len = skill_table
        .record_count
        .checked_mul(SKILL_RECORD_SIZE)
        .context("skill table byte length overflow")?;
    let skill_table_end = skill_table_offset
        .checked_add(skill_table_len)
        .context("skill table end offset overflow")?;
    let skill_table_bytes = pe_data
        .get(skill_table_offset..skill_table_end)
        .context("skill table exceeds PE data")?;

    // Extract all available client language arrays. English is still used as the
    // canonical row-selection language; output strings are emitted as language maps.
    let file_ids = pe.language_file_ids(
        &pe_data,
        CLIENT_TEXT_FILE_ID_TABLE_VA,
        CLIENT_TEXT_FILES_PER_LANGUAGE,
        0,
    )?;
    let localized_file_ids = CLIENT_LANGUAGE_CODES
        .iter()
        .map(|(language_index, code)| {
            pe.language_file_ids(
                &pe_data,
                CLIENT_TEXT_FILE_ID_TABLE_VA,
                CLIENT_TEXT_FILES_PER_LANGUAGE,
                *language_index,
            )
            .map(|ids| (*code, ids))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    // Cache to hold decompressed text file records
    let mut decompressed_cache = BTreeMap::<u32, BTreeMap<u32, String>>::new();
    fs::create_dir_all(images_dir).with_context(|| format!("creating {}", images_dir.display()))?;
    if let Some(images_hd_dir) = images_hd_dir {
        fs::create_dir_all(images_hd_dir)
            .with_context(|| format!("creating {}", images_hd_dir.display()))?;
    }

    // Helper to retrieve and decompress a record
    let mut get_text_record = |file: &mut File,
                               file_ids: &[Option<u32>],
                               file_index: usize,
                               record_index: u32|
     -> anyhow::Result<Option<String>> {
        let Some(file_id) = file_ids.get(file_index).and_then(|file_id| *file_id) else {
            return Ok(None);
        };
        if let std::collections::btree_map::Entry::Vacant(entry) = decompressed_cache.entry(file_id)
        {
            let Some(mft_index) = lookup_mft_index_for_file_id(file_id, &hash_to_mft) else {
                return Ok(None);
            };
            let Some(mft_entry) = mft_entry_by_index(&mft_entries, mft_index) else {
                return Ok(None);
            };
            let entry_bytes = read_dat_entry_from_file(file, metadata.len(), mft_entry)
                .with_context(|| {
                    format!(
                        "reading text file {file_id} from MFT entry {}",
                        mft_entry.index
                    )
                })?;
            let records = text_records::parse_text_record_map(&entry_bytes)
                .with_context(|| format!("parsing text records from file {file_id}"))?
                .into_iter()
                .map(|(record_index, text)| (record_index, clean_text(&text)))
                .collect();
            entry.insert(records);
        }
        Ok(decompressed_cache
            .get(&file_id)
            .and_then(|records| records.get(&record_index))
            .cloned())
    };

    let mut selected_indices = BTreeSet::new();
    let mut base_names = BTreeSet::new();

    // Scan all skill table records
    for i in 0..skill_table.record_count {
        let row_bytes = &skill_table_bytes[i * SKILL_RECORD_SIZE..(i + 1) * SKILL_RECORD_SIZE];
        let name_string_id = u32::from_le_bytes([
            row_bytes[0x98],
            row_bytes[0x99],
            row_bytes[0x9A],
            row_bytes[0x9B],
        ]);
        if name_string_id == 0 {
            continue;
        }

        let campaign_code = u32::from_le_bytes([
            row_bytes[0x08],
            row_bytes[0x09],
            row_bytes[0x0A],
            row_bytes[0x0B],
        ]);
        let campaign = campaign_name(campaign_code);
        let title_track_code = u16::from_le_bytes([row_bytes[0x2A], row_bytes[0x2B]]);
        let effective_campaign = match title_track_code {
            5 | 6 => "factions",
            _ => campaign,
        };

        let flags_val = u32::from_le_bytes([
            row_bytes[0x10],
            row_bytes[0x11],
            row_bytes[0x12],
            row_bytes[0x13],
        ]);
        let pvp = (flags_val & SKILL_FLAG_PVP) != 0;
        let profession_code = row_bytes[0x28];
        let skill_equip_type_code = row_bytes[0x33];

        let is_equippable = matches!(
            effective_campaign,
            "core" | "prophecies" | "factions" | "nightfall" | "eye_of_the_north"
        ) && !pvp
            && skill_equip_type_code == 1
            && ((1..=10).contains(&profession_code)
                || (effective_campaign == "eye_of_the_north" && profession_code == 0));

        if is_equippable {
            selected_indices.insert(i);
            if let Some(name) = get_text_record(
                &mut file,
                &file_ids,
                (name_string_id / TEXT_RECORDS_PER_FILE) as usize,
                name_string_id % TEXT_RECORDS_PER_FILE,
            )? {
                base_names.insert(name);
            }
        }
    }

    // Recover hidden Nightfall skills
    for i in 0..skill_table.record_count {
        let row_bytes = &skill_table_bytes[i * SKILL_RECORD_SIZE..(i + 1) * SKILL_RECORD_SIZE];
        let name_string_id = u32::from_le_bytes([
            row_bytes[0x98],
            row_bytes[0x99],
            row_bytes[0x9A],
            row_bytes[0x9B],
        ]);
        if name_string_id == 0 || selected_indices.contains(&i) {
            continue;
        }

        let campaign_code = u32::from_le_bytes([
            row_bytes[0x08],
            row_bytes[0x09],
            row_bytes[0x0A],
            row_bytes[0x0B],
        ]);
        let campaign = campaign_name(campaign_code);
        let flags_val = u32::from_le_bytes([
            row_bytes[0x10],
            row_bytes[0x11],
            row_bytes[0x12],
            row_bytes[0x13],
        ]);
        let pvp = (flags_val & SKILL_FLAG_PVP) != 0;
        let elite = (flags_val & 0x4) != 0;
        let profession_code = row_bytes[0x28];
        let skill_equip_type_code = row_bytes[0x33];

        let name_file_index = (name_string_id / TEXT_RECORDS_PER_FILE) as usize;
        let name_record_index = name_string_id % TEXT_RECORDS_PER_FILE;

        if campaign == "nightfall"
            && !pvp
            && !elite
            && (1..=10).contains(&profession_code)
            && skill_equip_type_code == 2
            && name_file_index == 26
            && let Some(name) =
                get_text_record(&mut file, &file_ids, name_file_index, name_record_index)?
            && name != "REMOVE"
            && !base_names.contains(&name)
        {
            selected_indices.insert(i);
        }
    }

    let mut extracted_skills = Vec::new();

    for index in selected_indices {
        let row_bytes =
            &skill_table_bytes[index * SKILL_RECORD_SIZE..(index + 1) * SKILL_RECORD_SIZE];
        let name_string_id = u32::from_le_bytes([
            row_bytes[0x98],
            row_bytes[0x99],
            row_bytes[0x9A],
            row_bytes[0x9B],
        ]);
        let description_string_id = u32::from_le_bytes([
            row_bytes[0xA0],
            row_bytes[0xA1],
            row_bytes[0xA2],
            row_bytes[0xA3],
        ]);

        let name_file_index = (name_string_id / TEXT_RECORDS_PER_FILE) as usize;
        let name_record_index = name_string_id % TEXT_RECORDS_PER_FILE;
        let desc_file_index = (description_string_id / TEXT_RECORDS_PER_FILE) as usize;
        let desc_record_index = description_string_id % TEXT_RECORDS_PER_FILE;

        let mut name = BTreeMap::new();
        let mut description = BTreeMap::new();
        for (code, language_file_ids) in &localized_file_ids {
            if let Some(text) = get_text_record(
                &mut file,
                language_file_ids,
                name_file_index,
                name_record_index,
            )? {
                name.insert((*code).to_string(), text);
            }
            if let Some(text) = get_text_record(
                &mut file,
                language_file_ids,
                desc_file_index,
                desc_record_index,
            )? {
                description.insert((*code).to_string(), text);
            }
        }

        let icon_texture_hash = u32::from_le_bytes([
            row_bytes[0x8C],
            row_bytes[0x8D],
            row_bytes[0x8E],
            row_bytes[0x8F],
        ]);

        let campaign_code = u32::from_le_bytes([
            row_bytes[0x08],
            row_bytes[0x09],
            row_bytes[0x0A],
            row_bytes[0x0B],
        ]);
        let campaign = campaign_name(campaign_code);
        let title_track_code = u16::from_le_bytes([row_bytes[0x2A], row_bytes[0x2B]]);
        let effective_campaign = match title_track_code {
            5 | 6 => "factions",
            _ => campaign,
        };

        let flags_val = u32::from_le_bytes([
            row_bytes[0x10],
            row_bytes[0x11],
            row_bytes[0x12],
            row_bytes[0x13],
        ]);
        let touch_range = (flags_val & 0x2) != 0;
        let elite = (flags_val & 0x4) != 0;
        let half_range = (flags_val & 0x8) != 0;
        let stacking = (flags_val & 0x10000) != 0;
        let non_stacking = (flags_val & 0x20000) != 0;
        let pvp = (flags_val & SKILL_FLAG_PVP) != 0;
        let pve = (flags_val & SKILL_FLAG_PVE) != 0;
        let playable = (flags_val & SKILL_FLAG_NOT_PLAYABLE) == 0;

        let profession_code = row_bytes[0x28];
        let type_code = u32::from_le_bytes([
            row_bytes[0x0C],
            row_bytes[0x0D],
            row_bytes[0x0E],
            row_bytes[0x0F],
        ]);
        let energy_cost_encoded = row_bytes[0x35];
        let skill_equip_type_code = row_bytes[0x33];

        let icon_hd_texture_hash = u32::from_le_bytes([
            row_bytes[0x90],
            row_bytes[0x91],
            row_bytes[0x92],
            row_bytes[0x93],
        ]);

        export_skill_icon(
            &mut file,
            metadata.len(),
            icon_texture_hash,
            &hash_to_mft,
            &mft_entries,
            &images_dir.join(format!("{index}.png")),
        );
        if icon_hd_texture_hash != 0
            && let Some(images_hd_dir) = images_hd_dir
        {
            export_skill_icon(
                &mut file,
                metadata.len(),
                icon_hd_texture_hash,
                &hash_to_mft,
                &mft_entries,
                &images_hd_dir.join(format!("{index}.png")),
            );
        }

        extracted_skills.push(ExtractedSkill {
            id: index as u32,
            name,
            description,
            campaign: effective_campaign.to_string(),
            profession: profession_name(profession_code).to_string(),
            profession_code,
            attribute_code: row_bytes[0x29],
            skill_type: skill_type_name(type_code).to_string(),
            type_code,
            elite,
            costs: SkillCosts {
                energy: decoded_energy_cost(energy_cost_encoded),
                energy_encoded: energy_cost_encoded,
                health: row_bytes[0x36],
                adrenaline: u32::from_le_bytes([
                    row_bytes[0x38],
                    row_bytes[0x39],
                    row_bytes[0x3A],
                    row_bytes[0x3B],
                ]),
                overcast: row_bytes[0x34],
            },
            timing: SkillTiming {
                activation_seconds: f32::from_le_bytes([
                    row_bytes[0x3C],
                    row_bytes[0x3D],
                    row_bytes[0x3E],
                    row_bytes[0x3F],
                ]),
                aftercast_seconds: f32::from_le_bytes([
                    row_bytes[0x40],
                    row_bytes[0x41],
                    row_bytes[0x42],
                    row_bytes[0x43],
                ]),
                recharge_seconds: u32::from_le_bytes([
                    row_bytes[0x4C],
                    row_bytes[0x4D],
                    row_bytes[0x4E],
                    row_bytes[0x4F],
                ]),
                duration_0_attribute: u32::from_le_bytes([
                    row_bytes[0x44],
                    row_bytes[0x45],
                    row_bytes[0x46],
                    row_bytes[0x47],
                ]),
                duration_15_attribute: u32::from_le_bytes([
                    row_bytes[0x48],
                    row_bytes[0x49],
                    row_bytes[0x4A],
                    row_bytes[0x4B],
                ]),
            },
            target_code: row_bytes[0x31],
            aoe_range: f32::from_le_bytes([
                row_bytes[0x6C],
                row_bytes[0x6D],
                row_bytes[0x6E],
                row_bytes[0x6F],
            ]),
            constant_effect: f32::from_le_bytes([
                row_bytes[0x70],
                row_bytes[0x71],
                row_bytes[0x72],
                row_bytes[0x73],
            ]),
            skill_equip_type_code,
            flags: SkillFlags {
                touch_range,
                elite,
                half_range,
                stacking,
                non_stacking,
                pvp,
                pve,
                playable,
            },
        });
    }

    let mut campaigns_stats = BTreeMap::new();
    for campaign in &[
        "core",
        "prophecies",
        "factions",
        "nightfall",
        "eye_of_the_north",
    ] {
        let (non_elite, elite) = extracted_skills
            .iter()
            .filter(|s| &s.campaign == campaign)
            .fold(
                (0, 0),
                |(ne, el), s| if s.elite { (ne, el + 1) } else { (ne + 1, el) },
            );
        campaigns_stats.insert(
            campaign.to_string(),
            OutputCampaignStats {
                non_elite,
                elite,
                total: non_elite + elite,
            },
        );
    }

    let final_output = OutputManifest {
        generated_at: Utc::now().to_rfc3339(),
        counts: OutputCounts {
            skills: extracted_skills.len(),
            campaigns: campaigns_stats,
        },
        skills: extracted_skills,
    };

    let serialized = serde_json::to_string_pretty(&final_output)?;
    fs::write(out_path, serialized)?;
    Ok(())
}
