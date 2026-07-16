use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};

use super::{
    CollectorEntry, CoverageAccumulator, CoverageEntry, CoverageNpc, ItemCatalogEntry,
    ItemVendorEntry, ServiceNpcKey, SkillTrainerEntry,
};

pub(super) fn item_vendor_catalog(
    entries: BTreeMap<ServiceNpcKey, BTreeSet<ItemCatalogEntry>>,
    service_name_words: &BTreeMap<ServiceNpcKey, Vec<u16>>,
    localized_npc_names: &BTreeMap<Vec<u16>, BTreeMap<String, String>>,
) -> Result<Vec<ItemVendorEntry>> {
    entries
        .into_iter()
        .map(|(key, items)| {
            Ok(ItemVendorEntry {
                key,
                position: key.position(),
                localized: localized_service_name(key, service_name_words, localized_npc_names)?
                    .unwrap_or_default(),
                items: items.into_iter().collect(),
            })
        })
        .collect()
}

pub(super) fn build_coverage(
    collectors: &[CollectorEntry],
    merchants: &[ItemVendorEntry],
    crafters: &[ItemVendorEntry],
    skill_trainers: &[SkillTrainerEntry],
) -> Vec<CoverageEntry> {
    let mut maps = BTreeMap::<u32, CoverageAccumulator>::new();
    for entry in collectors {
        maps.entry(entry.key.map_id)
            .or_default()
            .collectors
            .insert(entry.key);
    }
    for entry in merchants {
        maps.entry(entry.key.map_id)
            .or_default()
            .merchants
            .insert(entry.key);
    }
    for entry in crafters {
        maps.entry(entry.key.map_id)
            .or_default()
            .crafters
            .insert(entry.key);
    }
    for entry in skill_trainers {
        maps.entry(entry.key.map_id)
            .or_default()
            .skill_trainers
            .insert(entry.key);
    }
    maps.into_iter()
        .map(|(map_id, entry)| CoverageEntry {
            map_id,
            collectors: entry
                .collectors
                .into_iter()
                .map(CoverageNpc::from)
                .collect(),
            merchants: entry.merchants.into_iter().map(CoverageNpc::from).collect(),
            crafters: entry.crafters.into_iter().map(CoverageNpc::from).collect(),
            skill_trainers: entry
                .skill_trainers
                .into_iter()
                .map(CoverageNpc::from)
                .collect(),
        })
        .collect()
}

pub(super) fn localized_service_name(
    service_npc: ServiceNpcKey,
    service_name_words: &BTreeMap<ServiceNpcKey, Vec<u16>>,
    localized_npc_names: &BTreeMap<Vec<u16>, BTreeMap<String, String>>,
) -> Result<Option<BTreeMap<String, String>>> {
    let Some(words) = service_name_words.get(&service_npc) else {
        return Ok(None);
    };
    localized_npc_names
        .get(words)
        .cloned()
        .map(Some)
        .with_context(|| format!("service NPC {:?} name has no localized text", service_npc))
}
