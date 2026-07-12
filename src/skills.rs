mod extraction;
mod icons;
mod table;

use serde::Serialize;
use std::collections::BTreeMap;

pub(crate) use extraction::extract_skills_to_model_file_dirs;

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

#[derive(Serialize)]
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
    generated_at: String,
    counts: OutputCounts,
    skills: Vec<ExtractedSkill>,
}

fn decoded_energy_cost(encoded: u8) -> u32 {
    match encoded {
        11 => 15,
        12 => 25,
        other => other as u32,
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
