use super::{
    ExtractedSkill, OutputCampaignStats, OutputCounts, OutputManifest, SKILL_FLAG_NOT_PLAYABLE,
    SKILL_FLAG_PVE, SKILL_FLAG_PVP, SKILL_OUTPUT_SCHEMA_VERSION, SkillCosts, SkillFlags,
    SkillTiming, campaign_name, decoded_energy_cost,
    icons::export_skill_icon,
    overcast_cost, profession_name, skill_type_name,
    table::{SKILL_RECORD_SIZE, locate_skill_table},
    validate_skill_distribution,
};

use anyhow::{Context, bail};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use crate::{
    dat::DatArchive,
    io_util::write_json,
    pe::PeImage,
    text::{catalog::LocalizedTextReader, clean_display_text},
};

fn skill_row(skill_table_bytes: &[u8], index: usize) -> Option<&[u8]> {
    let start = index.checked_mul(SKILL_RECORD_SIZE)?;
    let end = start.checked_add(SKILL_RECORD_SIZE)?;
    skill_table_bytes.get(start..end)
}

fn skill_u32(row_bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        row_bytes[offset],
        row_bytes[offset + 1],
        row_bytes[offset + 2],
        row_bytes[offset + 3],
    ])
}

fn select_skill_indices(skill_table_bytes: &[u8]) -> anyhow::Result<BTreeSet<usize>> {
    if !skill_table_bytes.len().is_multiple_of(SKILL_RECORD_SIZE) {
        bail!("skill table length is not a multiple of {SKILL_RECORD_SIZE}");
    }

    let mut selected_indices = BTreeSet::new();
    for (index, row_bytes) in skill_table_bytes
        .chunks_exact(SKILL_RECORD_SIZE)
        .enumerate()
    {
        if skill_u32(row_bytes, 0) != index as u32 {
            bail!("skill table row {index} has a mismatched skill id");
        }

        let flags = skill_u32(row_bytes, 0x10);
        let standard = flags & SKILL_FLAG_PVP == 0 && row_bytes[0x33] == 1;
        let base_index = skill_u32(row_bytes, 0x2c) as usize;
        let current_pvp_variant = flags & SKILL_FLAG_PVP != 0
            && row_bytes[0x33] == 0
            && skill_row(skill_table_bytes, base_index).is_some_and(|base_row| {
                skill_u32(base_row, 0x10) & SKILL_FLAG_PVP == 0
                    && base_row[0x33] == 1
                    && skill_u32(base_row, 0x2c) as usize == index
            });
        if standard || current_pvp_variant {
            selected_indices.insert(index);
        }
    }

    Ok(selected_indices)
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
    let mut archive = DatArchive::open(gw_dat_path)?;
    let pe_data = archive.client_pe_data()?;
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

    let compact_seeds = BTreeMap::new();
    let decoded_records = BTreeMap::new();
    let mut text_reader = LocalizedTextReader::new(
        &mut archive,
        &pe_data,
        &pe,
        &compact_seeds,
        &decoded_records,
    )?;

    let selected_indices = select_skill_indices(skill_table_bytes)?;

    let mut extracted_skills = Vec::new();
    let mut icon_jobs = Vec::new();

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

        let name = text_reader
            .text(name_string_id)?
            .into_iter()
            .filter_map(|(code, text)| {
                let text = clean_display_text(&text);
                (!text.is_empty()).then_some((code, text))
            })
            .collect();
        let description = text_reader
            .text(description_string_id)?
            .into_iter()
            .filter_map(|(code, text)| {
                let text = clean_display_text(&text);
                (!text.is_empty()).then_some((code, text))
            })
            .collect();

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

        icon_jobs.push((index, icon_texture_hash, icon_hd_texture_hash));

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
                overcast: overcast_cost(flags_val, row_bytes[0x34]),
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
    validate_skill_distribution(&campaigns_stats, extracted_skills.len())?;

    drop(text_reader);
    fs::create_dir_all(images_dir).with_context(|| format!("creating {}", images_dir.display()))?;
    if let Some(images_hd_dir) = images_hd_dir {
        fs::create_dir_all(images_hd_dir)
            .with_context(|| format!("creating {}", images_hd_dir.display()))?;
    }
    for (index, icon_texture_hash, icon_hd_texture_hash) in icon_jobs {
        let icon_path = images_dir.join(format!("{index}.png"));
        if !export_skill_icon(&mut archive, icon_texture_hash, &icon_path)
            .with_context(|| format!("exporting skill {index} icon"))?
        {
            bail!("skill {index} icon file id {icon_texture_hash} is missing from the DAT index");
        }
        if icon_hd_texture_hash != 0
            && let Some(images_hd_dir) = images_hd_dir
        {
            let icon_path = images_hd_dir.join(format!("{index}.png"));
            if !export_skill_icon(&mut archive, icon_hd_texture_hash, &icon_path)
                .with_context(|| format!("exporting skill {index} HD icon"))?
            {
                bail!(
                    "skill {index} HD icon file id {icon_hd_texture_hash} is missing from the DAT index"
                );
            }
        }
    }

    let final_output = OutputManifest {
        schema_version: SKILL_OUTPUT_SCHEMA_VERSION,
        counts: OutputCounts {
            skills: extracted_skills.len(),
            campaigns: campaigns_stats,
        },
        skills: extracted_skills,
    };
    write_json(out_path, &final_output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_u32(row: &mut [u8], offset: usize, value: u32) {
        row[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn selects_standard_skills_and_their_current_pvp_variants() {
        let mut table = vec![0; SKILL_RECORD_SIZE * 6];
        for (index, row) in table.chunks_exact_mut(SKILL_RECORD_SIZE).enumerate() {
            set_u32(row, 0, index as u32);
        }

        let base = &mut table[SKILL_RECORD_SIZE..SKILL_RECORD_SIZE * 2];
        base[0x33] = 1;
        set_u32(base, 0x2c, 3);

        let hidden = &mut table[SKILL_RECORD_SIZE * 2..SKILL_RECORD_SIZE * 3];
        hidden[0x33] = 2;

        let pvp = &mut table[SKILL_RECORD_SIZE * 3..SKILL_RECORD_SIZE * 4];
        set_u32(pvp, 0x10, SKILL_FLAG_PVP);
        set_u32(pvp, 0x2c, 1);

        let stale_pvp = &mut table[SKILL_RECORD_SIZE * 4..SKILL_RECORD_SIZE * 5];
        set_u32(stale_pvp, 0x10, SKILL_FLAG_PVP);

        let special = &mut table[SKILL_RECORD_SIZE * 5..];
        set_u32(special, 0x10, SKILL_FLAG_NOT_PLAYABLE);
        special[0x33] = 1;

        assert_eq!(
            select_skill_indices(&table)
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![1, 3, 5]
        );
    }
}
