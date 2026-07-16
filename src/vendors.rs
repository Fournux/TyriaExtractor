mod capture;
mod catalog;
#[cfg(test)]
mod tests;

pub(crate) use capture::captured_npc_name_words;
use capture::observe_npc_packet;
use catalog::{build_coverage, item_vendor_catalog, localized_service_name};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use crate::io_util::write_json;

#[derive(Debug, Deserialize)]
struct PacketRow {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    session_id: u64,
    #[serde(default)]
    header: u32,
    #[serde(default)]
    raw_hex: String,
    #[serde(default)]
    merchant_agent_id: u32,
    #[serde(default)]
    npc_model_id: Option<u32>,
    #[serde(default)]
    transaction_service: u32,
    #[serde(default)]
    reward_count: usize,
    #[serde(default)]
    captured_reward_count: usize,
    #[serde(default)]
    required_item: Option<RequiredItem>,
    #[serde(default)]
    rewards: Vec<RewardItem>,
    #[serde(default)]
    capture_complete: bool,
    #[serde(default)]
    entry_count: usize,
    #[serde(default)]
    captured_entry_count: usize,
    #[serde(default)]
    entries: Vec<ServiceEntry>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct RequiredItem {
    model_id: u32,
    quantity: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct RewardItem {
    model_id: Option<u32>,
    model_file_id: Option<u32>,
    item_type: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
struct ServiceEntry {
    item_id: Option<u32>,
    model_id: Option<u32>,
    model_file_id: Option<u32>,
    item_type: Option<u32>,
    base_value: Option<u32>,
    skill_id: Option<u32>,
    availability_flags_raw: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ServiceNpcKey {
    map_id: u32,
    npc_model_id: u32,
    #[serde(skip)]
    position_x_bits: u32,
    #[serde(skip)]
    position_y_bits: u32,
}

impl ServiceNpcKey {
    fn position(self) -> Position {
        Position {
            x: f32::from_bits(self.position_x_bits),
            y: f32::from_bits(self.position_y_bits),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct Position {
    x: f32,
    y: f32,
}

#[derive(Debug, Serialize)]
struct CoverageNpc {
    npc_model_id: u32,
    position: Position,
}

impl From<ServiceNpcKey> for CoverageNpc {
    fn from(key: ServiceNpcKey) -> Self {
        Self {
            npc_model_id: key.npc_model_id,
            position: key.position(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct CollectorOffer {
    required_item: RequiredItem,
    rewards: Vec<ResolvedRewardItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ResolvedRewardItem {
    model_id: u32,
    model_file_id: u32,
    item_type: u32,
}

#[derive(Debug, Serialize)]
struct CollectorEntry {
    #[serde(flatten)]
    key: ServiceNpcKey,
    position: Position,
    #[serde(flatten)]
    localized: BTreeMap<String, String>,
    offers: Vec<CollectorOffer>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ItemCatalogEntry {
    model_id: u32,
    model_file_id: u32,
    item_type: u32,
    base_value: u32,
}

#[derive(Debug, Serialize)]
struct ItemVendorEntry {
    #[serde(flatten)]
    key: ServiceNpcKey,
    position: Position,
    #[serde(flatten)]
    localized: BTreeMap<String, String>,
    items: Vec<ItemCatalogEntry>,
}

#[derive(Debug, Serialize)]
struct SkillTrainerEntry {
    #[serde(flatten)]
    key: ServiceNpcKey,
    position: Position,
    #[serde(flatten)]
    localized: BTreeMap<String, String>,
    skills: Vec<TrainerSkill>,
}

#[derive(Debug, Serialize)]
struct TrainerSkill {
    skill_id: u32,
    availability_flags_raw: Vec<u32>,
}

#[derive(Debug, Default)]
struct CoverageAccumulator {
    collectors: BTreeSet<ServiceNpcKey>,
    merchants: BTreeSet<ServiceNpcKey>,
    crafters: BTreeSet<ServiceNpcKey>,
    skill_trainers: BTreeSet<ServiceNpcKey>,
}

#[derive(Debug, Serialize)]
struct CoverageEntry {
    map_id: u32,
    collectors: Vec<CoverageNpc>,
    merchants: Vec<CoverageNpc>,
    crafters: Vec<CoverageNpc>,
    skill_trainers: Vec<CoverageNpc>,
}

pub(crate) fn extract_vendor_catalogs_from_packet_log(
    path: &Path,
    out_dir: &Path,
    localized_npc_names: &BTreeMap<Vec<u16>, BTreeMap<String, String>>,
) -> Result<()> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut current_maps = BTreeMap::<u64, u32>::new();
    let mut agent_npcs = BTreeMap::<(u64, u32), ServiceNpcKey>::new();
    let mut agent_names = BTreeMap::<(u64, u32), Vec<u16>>::new();
    let mut collector_offers = BTreeMap::<ServiceNpcKey, BTreeSet<CollectorOffer>>::new();
    let mut service_name_words = BTreeMap::<ServiceNpcKey, Vec<u16>>::new();
    let mut merchant_items = BTreeMap::<ServiceNpcKey, BTreeSet<ItemCatalogEntry>>::new();
    let mut crafter_products = BTreeMap::<ServiceNpcKey, BTreeSet<ItemCatalogEntry>>::new();
    let mut trainer_skills = BTreeMap::<ServiceNpcKey, BTreeMap<u32, BTreeSet<u32>>>::new();

    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line =
            line.with_context(|| format!("reading {} line {}", path.display(), line_index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let row: PacketRow = serde_json::from_str(&line)
            .with_context(|| format!("parsing {} line {}", path.display(), line_index + 1))?;
        if row.kind == "world_packet" {
            observe_npc_packet(
                &row,
                &mut current_maps,
                &mut agent_npcs,
                &mut agent_names,
                &mut service_name_words,
            )?;
            continue;
        }
        if !matches!(
            row.kind.as_str(),
            "collector_offers" | "merchant_items" | "crafter_products" | "skill_trainer_skills"
        ) {
            continue;
        }
        let service_npc = agent_npcs
            .get(&(row.session_id, row.merchant_agent_id))
            .copied()
            .with_context(|| {
                format!(
                    "{} line {} {} agent {} has no captured spawn position",
                    path.display(),
                    line_index + 1,
                    row.kind,
                    row.merchant_agent_id
                )
            })?;
        if row
            .npc_model_id
            .filter(|value| *value != 0)
            .is_some_and(|npc_model_id| npc_model_id != service_npc.npc_model_id)
        {
            bail!(
                "{} line {} {} agent {} resolves to NPC model {} but captured row says {}",
                path.display(),
                line_index + 1,
                row.kind,
                row.merchant_agent_id,
                service_npc.npc_model_id,
                row.npc_model_id.unwrap_or_default()
            );
        }

        if row.kind == "collector_offers"
            && row.transaction_service == 2
            && row.reward_count != 0
            && row.reward_count == row.captured_reward_count
            && row.reward_count == row.rewards.len()
        {
            let Some(required_item) = row.required_item.filter(|item| {
                item.model_id != 0 && (1..=u32::from(u16::MAX)).contains(&item.quantity)
            }) else {
                continue;
            };
            let Some(rewards) = row
                .rewards
                .into_iter()
                .map(|item| {
                    Some(ResolvedRewardItem {
                        model_id: item.model_id.filter(|value| *value != 0)?,
                        model_file_id: item.model_file_id.filter(|value| *value != 0)?,
                        item_type: item.item_type?,
                    })
                })
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            collector_offers
                .entry(service_npc)
                .or_default()
                .insert(CollectorOffer {
                    required_item,
                    rewards,
                });
            continue;
        }

        if !row.capture_complete
            || row.entry_count == 0
            || row.entry_count != row.captured_entry_count
            || row.entry_count != row.entries.len()
        {
            continue;
        }
        match (row.kind.as_str(), row.transaction_service) {
            ("merchant_items", 1) | ("crafter_products", 3) => {
                let Some(items) = row
                    .entries
                    .iter()
                    .map(|entry| {
                        entry.item_id.filter(|value| *value != 0)?;
                        Some(ItemCatalogEntry {
                            model_id: entry.model_id.filter(|value| *value != 0)?,
                            model_file_id: entry.model_file_id.filter(|value| *value != 0)?,
                            item_type: entry.item_type?,
                            base_value: entry.base_value?,
                        })
                    })
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                let catalog = if row.transaction_service == 1 {
                    &mut merchant_items
                } else {
                    &mut crafter_products
                };
                catalog.entry(service_npc).or_default().extend(items);
            }
            ("skill_trainer_skills", 10) => {
                let skills = trainer_skills.entry(service_npc).or_default();
                for entry in row.entries {
                    let Some(skill_id) = entry.skill_id.filter(|value| *value != 0) else {
                        continue;
                    };
                    skills
                        .entry(skill_id)
                        .or_default()
                        .insert(entry.availability_flags_raw.unwrap_or_default());
                }
            }
            _ => {}
        }
    }

    let collectors = collector_offers
        .into_iter()
        .map(|(key, offers)| {
            let localized = localized_service_name(
                key,
                &service_name_words,
                localized_npc_names,
            )?
            .with_context(|| {
                format!(
                    "collector {:?} has no captured AGENT_UPDATE_NPC_NAME; include a current capture for this instance",
                    key
                )
            })?;
            Ok(CollectorEntry {
                key,
                position: key.position(),
                localized,
                offers: offers.into_iter().collect(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let merchants = item_vendor_catalog(merchant_items, &service_name_words, localized_npc_names)?;
    let crafters = item_vendor_catalog(crafter_products, &service_name_words, localized_npc_names)?;
    let skill_trainers = trainer_skills
        .into_iter()
        .map(|(key, skills)| {
            Ok(SkillTrainerEntry {
                key,
                position: key.position(),
                localized: localized_service_name(key, &service_name_words, localized_npc_names)?
                    .unwrap_or_default(),
                skills: skills
                    .into_iter()
                    .map(|(skill_id, flags)| TrainerSkill {
                        skill_id,
                        availability_flags_raw: flags.into_iter().collect(),
                    })
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let coverage = build_coverage(&collectors, &merchants, &crafters, &skill_trainers);
    write_json(
        &out_dir.join("collectors").join("collectors.json"),
        &collectors,
    )?;
    write_json(
        &out_dir.join("merchants").join("merchants.json"),
        &merchants,
    )?;
    write_json(&out_dir.join("crafters").join("crafters.json"), &crafters)?;
    write_json(
        &out_dir.join("skill_trainers").join("skill_trainers.json"),
        &skill_trainers,
    )?;
    write_json(&out_dir.join("coverage.json"), &coverage)
}
