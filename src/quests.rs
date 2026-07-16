mod capture;
mod catalog;
mod packets;

use capture::*;
use catalog::*;
use packets::*;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use crate::{
    io_util::write_json,
    text::{
        TextReference,
        catalog::{LocalizedTextCatalog, resolve_localized_text_catalog},
        encoded_values_from_words, encoded_words_from_hex, for_each_localized_reference,
        text_references,
    },
};

#[derive(Debug)]
struct QuestDialogObservation {
    quest_id: u32,
    role: &'static str,
    agent_id: Option<u32>,
    npc_model_id: Option<u32>,
    map_id: Option<u32>,
    npc_encoded: Option<Vec<u16>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct QuestDialogKey {
    role: &'static str,
    agent_id: Option<u32>,
    npc_model_id: Option<u32>,
    npc_encoded: Option<Vec<u16>>,
}

#[derive(Debug, Default)]
struct QuestDialogAccumulator {
    map_ids: BTreeSet<u32>,
}

#[derive(Clone, Debug, Default)]
struct EncodedQuestFields {
    location: Option<Vec<u16>>,
    name: Option<Vec<u16>>,
    npc: Option<Vec<u16>>,
    description: Option<Vec<u16>>,
    objectives: Option<Vec<u16>>,
}

#[derive(Debug, Serialize)]
struct QuestStep {
    #[serde(flatten)]
    localized: BTreeMap<String, String>,
}

#[derive(Debug, Default, Serialize)]
struct QuestRewards {
    #[serde(skip_serializing_if = "Option::is_none")]
    experience: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gold: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    items: Vec<QuestRewardItem>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    skills: Vec<QuestRewardSkill>,
}

impl QuestRewards {
    fn is_empty(&self) -> bool {
        self.experience.is_none()
            && self.gold.is_none()
            && self.items.is_empty()
            && self.skills.is_empty()
    }
}

#[derive(Debug, Serialize)]
struct QuestRewardItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    model_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_file_id: Option<u32>,
    #[serde(flatten)]
    localized: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct QuestRewardSkill {
    #[serde(flatten)]
    localized: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct QuestDialogEvidence {
    role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    npc_model_id: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    map_ids: Vec<u32>,
    #[serde(flatten)]
    localized: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct QuestCatalogEntry {
    quest_id: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    origin_map_ids: Vec<u32>,
    #[serde(flatten)]
    localized: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    observed_steps: Vec<QuestStep>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    observed_step_sequences: Vec<Vec<QuestStep>>,
    #[serde(skip_serializing_if = "QuestRewards::is_empty")]
    rewards: QuestRewards,
    #[serde(rename = "quest_npcs", skip_serializing_if = "Vec::is_empty")]
    dialogs: Vec<QuestDialogEvidence>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RewardItemModel {
    model_id: u32,
    model_file_id: Option<u32>,
}

#[derive(Debug, Default)]
struct RewardItemModelLookup {
    unique: BTreeMap<(u32, u64), RewardItemModel>,
    observations: BTreeMap<(u32, u64), Vec<(CapturePoint, RewardItemModel)>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct StepObservation {
    text_ref: TextReference,
    encoded_words: Vec<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CapturePoint {
    session_id: u64,
    ts_ms: u128,
}

#[derive(Debug)]
struct QuestAccumulator {
    origin_map_ids: BTreeSet<u32>,
    encoded: EncodedQuestFields,
    steps: BTreeSet<StepObservation>,
    step_sequences: BTreeSet<Vec<StepObservation>>,
    dialogs: BTreeMap<QuestDialogKey, QuestDialogAccumulator>,
    description_observed: bool,
    reward_completions: BTreeSet<CapturePoint>,
}

impl QuestAccumulator {
    fn new(row: &QuestSnapshotRow) -> Self {
        let mut origin_map_ids = BTreeSet::new();
        if let Some(map_id) = row.map_from.filter(|map_id| *map_id != 0) {
            origin_map_ids.insert(map_id);
        }
        Self {
            origin_map_ids,
            encoded: EncodedQuestFields::default(),
            steps: BTreeSet::new(),
            step_sequences: BTreeSet::new(),
            dialogs: BTreeMap::new(),
            description_observed: false,
            reward_completions: BTreeSet::new(),
        }
    }

    fn observe(&mut self, row: QuestSnapshotRow) {
        if let Some(map_from) = row.map_from.filter(|map_id| *map_id != 0) {
            self.origin_map_ids.insert(map_from);
        }
        update_encoded(&mut self.encoded.location, row.location_encoded);
        update_encoded(&mut self.encoded.name, row.name_encoded);
        update_encoded(&mut self.encoded.npc, row.npc_encoded);
        update_encoded(&mut self.encoded.description, row.description_encoded);
        let Some(observed_objectives) = row.objectives_encoded else {
            return;
        };
        let sequence = objective_steps(&observed_objectives);
        self.encoded.objectives = Some(observed_objectives);
        for step in &sequence {
            self.steps.insert(step.clone());
        }
        if !sequence.is_empty() {
            self.step_sequences.insert(sequence);
        }
    }

    fn observe_dialog(&mut self, observation: QuestDialogObservation) {
        let key = QuestDialogKey {
            role: observation.role,
            agent_id: observation
                .npc_model_id
                .is_none()
                .then_some(observation.agent_id)
                .flatten(),
            npc_model_id: observation.npc_model_id,
            npc_encoded: observation.npc_encoded,
        };
        let dialog = self.dialogs.entry(key).or_default();
        if let Some(map_id) = observation.map_id {
            dialog.map_ids.insert(map_id);
        }
    }
}

fn update_encoded(current: &mut Option<Vec<u16>>, observed: Option<Vec<u16>>) {
    if observed.as_ref().is_some_and(|words| !words.is_empty()) {
        *current = observed;
    }
}

pub(crate) fn extract_quests_from_packet_log(
    gw_dat_path: &Path,
    packet_log_path: &Path,
    item_log_path: Option<&Path>,
    out_path: &Path,
) -> Result<()> {
    let quests = read_quest_accumulators(packet_log_path)?;
    if quests.is_empty() {
        bail!(
            "{} contains no decodable quest observations",
            packet_log_path.display()
        );
    }
    ensure_descriptions_complete(&quests)?;

    let (text_ids, seeds) = collect_text_inputs(quests.values());
    let lookup = resolve_localized_text_catalog(
        gw_dat_path,
        text_ids.iter().copied(),
        &seeds,
        &BTreeMap::new(),
    )
    .with_context(|| format!("resolving quest text from {}", gw_dat_path.display()))?;

    let reward_item_models = item_log_path
        .map(read_reward_item_models)
        .transpose()?
        .unwrap_or_default();

    let catalog = quests
        .into_iter()
        .map(|(quest_id, quest)| build_catalog_entry(quest_id, quest, &lookup, &reward_item_models))
        .collect::<Vec<_>>();
    write_json(out_path, &catalog)
}

fn ensure_descriptions_complete(quests: &BTreeMap<u32, QuestAccumulator>) -> Result<()> {
    let missing = quests
        .iter()
        .filter_map(|(quest_id, quest)| (!quest.description_observed).then_some(*quest_id))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(
            "quest capture is incomplete: missing QUEST_DESCRIPTION (0x004C) for IDs {missing:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
fn words_to_hex(words: &[u16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(words.len() * 4);
    for byte in words.iter().flat_map(|word| word.to_le_bytes()) {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests;
