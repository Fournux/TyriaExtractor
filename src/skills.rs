mod extraction;
mod icons;
mod table;

use serde::Serialize;
use std::collections::BTreeMap;

pub(crate) use extraction::extract_skills_to_model_file_dirs;

const SKILL_FLAG_OVERCAST: u32 = 0x0000_0001;
const SKILL_FLAG_PVE: u32 = 0x0008_0000;
const SKILL_FLAG_PVP: u32 = 0x0040_0000;
const SKILL_FLAG_NOT_PLAYABLE: u32 = 0x0200_0000;

#[derive(Serialize)]
struct SkillTiming {
    activation_seconds: f32,
    aftercast_seconds: f32,
    recharge_seconds: u32,
    duration_0_attribute: u32,
    duration_15_attribute: u32,
}

#[derive(Serialize)]
struct SkillCosts {
    energy: u32,
    energy_encoded: u8,
    health: u8,
    adrenaline: u32,
    overcast: u8,
}

#[derive(Serialize)]
struct SkillFlags {
    touch_range: bool,
    elite: bool,
    half_range: bool,
    stacking: bool,
    non_stacking: bool,
    pvp: bool,
    pve: bool,
    playable: bool,
}

#[derive(Serialize)]
struct ExtractedSkill {
    id: u32,
    name: BTreeMap<String, String>,
    description: BTreeMap<String, String>,
    campaign: String,
    profession: String,
    profession_code: u8,
    attribute_code: u8,
    #[serde(rename = "type")]
    skill_type: String,
    type_code: u32,
    elite: bool,
    costs: SkillCosts,
    timing: SkillTiming,
    target_code: u8,
    aoe_range: f32,
    constant_effect: f32,
    skill_equip_type_code: u8,
    flags: SkillFlags,
}

#[derive(Debug, Serialize)]
struct OutputCampaignStats {
    non_elite: usize,
    elite: usize,
    total: usize,
}

#[derive(Serialize)]
struct OutputCounts {
    skills: usize,
    campaigns: BTreeMap<String, OutputCampaignStats>,
}

#[derive(Serialize)]
struct OutputManifest {
    schema_version: u32,
    counts: OutputCounts,
    skills: Vec<ExtractedSkill>,
}
const SKILL_OUTPUT_SCHEMA_VERSION: u32 = 1;
const EXPECTED_SKILL_TOTAL: usize = 1488;
const EXPECTED_SKILL_DISTRIBUTION: [(&str, usize, usize); 5] = [
    ("core", 233, 48),
    ("prophecies", 159, 70),
    ("factions", 292, 104),
    ("nightfall", 294, 125),
    ("eye_of_the_north", 160, 3),
];

fn validate_skill_distribution(
    campaigns: &BTreeMap<String, OutputCampaignStats>,
    total: usize,
) -> anyhow::Result<()> {
    for (campaign, expected_non_elite, expected_elite) in EXPECTED_SKILL_DISTRIBUTION {
        let actual = campaigns
            .get(campaign)
            .ok_or_else(|| anyhow::anyhow!("missing {campaign} skill statistics"))?;
        if actual.non_elite != expected_non_elite || actual.elite != expected_elite {
            anyhow::bail!(
                "{campaign} skill distribution is {}/{} instead of {expected_non_elite}/{expected_elite}",
                actual.non_elite,
                actual.elite
            );
        }
    }
    if total != EXPECTED_SKILL_TOTAL {
        anyhow::bail!("skill catalog contains {total} skills instead of {EXPECTED_SKILL_TOTAL}");
    }
    Ok(())
}

fn decoded_energy_cost(encoded: u8) -> u32 {
    match encoded {
        11 => 15,
        12 => 25,
        other => other as u32,
    }
}

fn overcast_cost(special_flags: u32, raw: u8) -> u8 {
    if special_flags & SKILL_FLAG_OVERCAST != 0 {
        raw
    } else {
        0
    }
}

fn campaign_name(code: u32) -> &'static str {
    match code {
        0 => "core",
        1 => "prophecies",
        2 => "factions",
        3 => "nightfall",
        4 => "eye_of_the_north",
        _ => "unknown",
    }
}

fn profession_name(code: u8) -> &'static str {
    match code {
        0 => "none_or_environment",
        1 => "warrior",
        2 => "ranger",
        3 => "monk",
        4 => "necromancer",
        5 => "mesmer",
        6 => "elementalist",
        7 => "assassin",
        8 => "ritualist",
        9 => "paragon",
        10 => "dervish",
        _ => "unknown",
    }
}

fn skill_type_name(code: u32) -> &'static str {
    match code {
        3 => "stance",
        4 => "hex",
        5 => "spell",
        6 => "enchantment",
        7 => "signet",
        14 => "attack",
        15 => "shout",
        21 => "trap",
        22 => "ritual",
        25 => "weapon_spell",
        26 => "form",
        27 => "chant",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_required_skill_distribution() {
        let campaigns = EXPECTED_SKILL_DISTRIBUTION
            .into_iter()
            .map(|(campaign, non_elite, elite)| {
                (
                    campaign.to_string(),
                    OutputCampaignStats {
                        non_elite,
                        elite,
                        total: non_elite + elite,
                    },
                )
            })
            .collect();
        validate_skill_distribution(&campaigns, EXPECTED_SKILL_TOTAL).unwrap();
    }

    #[test]
    fn rejects_incomplete_skill_distribution() {
        let campaigns = BTreeMap::from([(
            "core".to_string(),
            OutputCampaignStats {
                non_elite: 211,
                elite: 40,
                total: 251,
            },
        )]);
        let error = validate_skill_distribution(&campaigns, 251)
            .expect_err("incomplete skill catalog must fail");
        assert!(format!("{error:#}").contains("core skill distribution"));
    }

    #[test]
    fn only_reports_overcast_when_its_special_flag_is_set() {
        assert_eq!(overcast_cost(SKILL_FLAG_PVE, 99), 0);
        assert_eq!(overcast_cost(SKILL_FLAG_OVERCAST | SKILL_FLAG_PVE, 10), 10);
    }
}
