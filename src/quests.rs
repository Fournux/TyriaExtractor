use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use crate::{
    io_util::write_json,
    items::{RuntimeTextLookup, runtime_text_lookup_with_compact_seeds},
    text::{
        TextReference, apply_encoded_template, encoded_values_from_words, encoded_words_from_hex,
        hex_to_bytes, text_references,
    },
};

const MAX_PLAUSIBLE_QUEST_ID: u32 = u16::MAX as u32;
const REWARD_ITEM_CORRELATION_WINDOW_MS: u128 = 120_000;
const REWARD_DIALOG_LIFETIME_MS: u128 = 30 * 60 * 1_000;

#[derive(Debug, Deserialize)]
struct QuestSnapshotRow {
    quest_id: u32,
    #[serde(default)]
    map_from: Option<u32>,
    #[serde(
        default,
        rename = "location_enc_hex",
        deserialize_with = "deserialize_encoded_words"
    )]
    location_encoded: Option<Vec<u16>>,
    #[serde(
        default,
        rename = "name_enc_hex",
        deserialize_with = "deserialize_encoded_words"
    )]
    name_encoded: Option<Vec<u16>>,
    #[serde(
        default,
        rename = "npc_enc_hex",
        deserialize_with = "deserialize_encoded_words"
    )]
    npc_encoded: Option<Vec<u16>>,
    #[serde(
        default,
        rename = "description_enc_hex",
        deserialize_with = "deserialize_encoded_words"
    )]
    description_encoded: Option<Vec<u16>>,
    #[serde(
        default,
        rename = "objectives_enc_hex",
        deserialize_with = "deserialize_encoded_words"
    )]
    objectives_encoded: Option<Vec<u16>>,
}
fn deserialize_encoded_words<'de, D>(deserializer: D) -> Result<Option<Vec<u16>>, D::Error>
where
    D: Deserializer<'de>,
{
    let encoded = Option::<String>::deserialize(deserializer)?;
    encoded
        .map(|encoded| {
            encoded_words_from_hex(&encoded)
                .ok_or_else(|| serde::de::Error::custom("encoded quest text is not valid hex"))
        })
        .transpose()
        .map(|words| words.filter(|words| !words.is_empty()))
}

#[derive(Debug, Deserialize)]
struct QuestPacketRow {
    #[serde(default)]
    ts_ms: u128,
    #[serde(default)]
    session_id: u64,
    header: u32,
    raw_hex: String,
}

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

#[derive(Clone, Debug)]
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
    steps: Vec<StepObservation>,
    step_indexes: BTreeMap<(u32, u64), usize>,
    step_sequences: BTreeMap<Vec<(u32, u64)>, Vec<StepObservation>>,
    dialogs: BTreeMap<QuestDialogKey, QuestDialogAccumulator>,
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
            steps: Vec::new(),
            step_indexes: BTreeMap::new(),
            step_sequences: BTreeMap::new(),
            dialogs: BTreeMap::new(),
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
        let sequence_key = sequence
            .iter()
            .map(|step| (step.text_ref.id, step.text_ref.seed))
            .collect::<Vec<_>>();
        for step in &sequence {
            let key = (step.text_ref.id, step.text_ref.seed);
            if !self.step_indexes.contains_key(&key) {
                self.step_indexes.insert(key, self.steps.len());
                self.steps.push(step.clone());
            }
        }
        if !sequence.is_empty() {
            self.step_sequences.entry(sequence_key).or_insert(sequence);
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

    let (text_ids, seeds) = collect_text_inputs(quests.values());
    let lookup =
        runtime_text_lookup_with_compact_seeds(gw_dat_path, &text_ids, &seeds, &BTreeMap::new())
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

fn read_reward_item_models(path: &Path) -> Result<RewardItemModelLookup> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut lookup = RewardItemModelLookup::default();
    let mut candidates = BTreeMap::<(u32, u64), BTreeMap<u32, BTreeSet<u32>>>::new();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line =
            line.with_context(|| format!("reading {} line {}", path.display(), line_index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)
            .with_context(|| format!("parsing {} line {}", path.display(), line_index + 1))?;
        let Some(model_id) = value
            .get("model_id")
            .and_then(|value| value.as_u64())
            .and_then(|value| u32::try_from(value).ok())
            .filter(|model_id| !matches!(*model_id, 0 | u32::MAX))
        else {
            continue;
        };
        let Some(name_text_id) = value
            .get("name_text_id")
            .and_then(|value| value.as_u64())
            .and_then(|value| u32::try_from(value).ok())
        else {
            continue;
        };
        let Some(words) = value
            .get("enc_name_hex")
            .and_then(|value| value.as_str())
            .and_then(encoded_words_from_hex)
        else {
            continue;
        };
        let Some(name) = text_references(&words)
            .into_iter()
            .find(|text_ref| text_ref.id == name_text_id)
        else {
            continue;
        };
        let model_file_id = value
            .get("model_file_id")
            .and_then(|value| value.as_u64())
            .and_then(|value| u32::try_from(value).ok())
            .filter(|model_file_id| *model_file_id != u32::MAX);
        let file_ids = candidates
            .entry((name.id, name.seed))
            .or_default()
            .entry(model_id)
            .or_default();
        if let Some(model_file_id) = model_file_id {
            file_ids.insert(model_file_id);
        }
        if let Some(ts_ms) = value.get("ts_ms").and_then(|value| value.as_u64()) {
            let session_id = value
                .get("session_id")
                .and_then(|value| value.as_u64())
                .unwrap_or_default();
            lookup
                .observations
                .entry((name.id, name.seed))
                .or_default()
                .push((
                    CapturePoint {
                        session_id,
                        ts_ms: u128::from(ts_ms),
                    },
                    RewardItemModel {
                        model_id,
                        model_file_id,
                    },
                ));
        }
    }
    lookup.unique = candidates
        .into_iter()
        .filter_map(|(key, models)| {
            let (model_id, file_ids) = models.first_key_value()?;
            (models.len() == 1).then_some((
                key,
                RewardItemModel {
                    model_id: *model_id,
                    model_file_id: (file_ids.len() == 1)
                        .then(|| file_ids.first().copied())
                        .flatten(),
                },
            ))
        })
        .collect();
    Ok(lookup)
}

fn read_quest_accumulators(path: &Path) -> Result<BTreeMap<u32, QuestAccumulator>> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut quests = BTreeMap::<u32, QuestAccumulator>::new();
    let mut agents = BTreeMap::<(u64, u32), u32>::new();
    let mut npc_names = BTreeMap::<u32, Vec<u16>>::new();
    let mut dialog_senders = BTreeMap::<u64, u32>::new();
    let mut current_maps = BTreeMap::<u64, u32>::new();
    let mut dialogs = Vec::new();
    let mut open_reward_dialogs = BTreeMap::<(u64, u32), u128>::new();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line =
            line.with_context(|| format!("reading {} line {}", path.display(), line_index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)
            .with_context(|| format!("parsing {} line {}", path.display(), line_index + 1))?;
        let kind = value.get("kind").and_then(|kind| kind.as_str());
        if matches!(kind, Some("quest_status" | "status"))
            && value.get("status").and_then(|status| status.as_str())
                == Some("quest_hooks_installed")
        {
            let session_id = value
                .get("session_id")
                .and_then(|value| value.as_u64())
                .unwrap_or_default();
            agents.retain(|(session, _), _| *session != session_id);
            dialog_senders.remove(&session_id);
            open_reward_dialogs.retain(|(session, _), _| *session != session_id);
            current_maps.remove(&session_id);
            continue;
        }
        if kind == Some("quest_snapshot") {
            let row: QuestSnapshotRow = serde_json::from_value(value)
                .with_context(|| format!("decoding {} line {}", path.display(), line_index + 1))?;
            let quest_id = row.quest_id;
            quests
                .entry(quest_id)
                .or_insert_with(|| QuestAccumulator::new(&row))
                .observe(row);
            continue;
        }
        if kind != Some("quest_packet") {
            continue;
        }
        let packet: QuestPacketRow = serde_json::from_value(value)
            .with_context(|| format!("decoding {} line {}", path.display(), line_index + 1))?;
        if let Some(row) = parse_quest_packet(&packet).with_context(|| {
            format!(
                "decoding quest packet 0x{:04X} at {} line {}",
                packet.header,
                path.display(),
                line_index + 1
            )
        })? {
            let quest_id = row.quest_id;
            quests
                .entry(quest_id)
                .or_insert_with(|| QuestAccumulator::new(&row))
                .observe(row);
        }
        match packet.header {
            0x20 => {
                let (agent_id, npc_model_id) = parse_agent_spawned_packet(&packet)?;
                let key = (packet.session_id, agent_id);
                if let Some(npc_model_id) = npc_model_id {
                    agents.insert(key, npc_model_id);
                } else {
                    agents.remove(&key);
                }
            }
            0x21 => {
                let agent_id = parse_agent_despawned_packet(&packet)?;
                agents.remove(&(packet.session_id, agent_id));
                if dialog_senders.get(&packet.session_id) == Some(&agent_id) {
                    dialog_senders.remove(&packet.session_id);
                }
            }
            0x52 => {
                let quest_id = parse_quest_remove_id(&packet)?;
                if let Some(offered_at) = open_reward_dialogs.remove(&(packet.session_id, quest_id))
                    && packet.ts_ms >= offered_at
                    && packet.ts_ms.saturating_sub(offered_at) <= REWARD_DIALOG_LIFETIME_MS
                {
                    let seed = partial_quest_row(quest_id);
                    quests
                        .entry(quest_id)
                        .or_insert_with(|| QuestAccumulator::new(&seed))
                        .reward_completions
                        .insert(CapturePoint {
                            session_id: packet.session_id,
                            ts_ms: packet.ts_ms,
                        });
                }
            }
            0x56 => {
                let (npc_model_id, name) = parse_npc_update_properties_packet(&packet)?;
                if let Some(name) = name {
                    npc_names.insert(npc_model_id, name);
                }
            }
            0x7e => {
                if let Some((quest_id, dialog_type)) = parse_dialog_button_packet(&packet)?
                    && let Some(role) = dialog_role(dialog_type)
                {
                    if dialog_type == "reward" {
                        open_reward_dialogs.insert((packet.session_id, quest_id), packet.ts_ms);
                    }
                    let agent_id = dialog_senders.get(&packet.session_id).copied();
                    dialogs.push(QuestDialogObservation {
                        quest_id,
                        role,
                        agent_id,
                        npc_model_id: agent_id.and_then(|agent_id| {
                            agents.get(&(packet.session_id, agent_id)).copied()
                        }),
                        map_id: current_maps.get(&packet.session_id).copied(),
                        npc_encoded: None,
                    });
                }
            }
            0x81 => {
                dialog_senders.insert(packet.session_id, parse_dialog_sender_packet(&packet)?);
            }
            0x199 => {
                let map_id = parse_instance_load_info_packet(&packet)?;
                current_maps.insert(packet.session_id, map_id);
                agents.retain(|(session, _), _| *session != packet.session_id);
                dialog_senders.remove(&packet.session_id);
                open_reward_dialogs.retain(|(session, _), _| *session != packet.session_id);
            }
            _ => {}
        }
    }
    for mut dialog in dialogs {
        dialog.npc_encoded = dialog
            .npc_model_id
            .and_then(|npc_model_id| npc_names.get(&npc_model_id).cloned());
        let seed = partial_quest_row(dialog.quest_id);
        quests
            .entry(dialog.quest_id)
            .or_insert_with(|| QuestAccumulator::new(&seed))
            .observe_dialog(dialog);
    }
    Ok(quests)
}

fn parse_quest_packet(packet: &QuestPacketRow) -> Result<Option<QuestSnapshotRow>> {
    let row = match packet.header {
        0x49 => parse_quest_add_packet(packet).map(Some),
        0x4c => parse_quest_description_packet(packet).map(Some),
        0x50 => parse_quest_general_info_packet(packet).map(Some),
        0x54 => parse_quest_update_objectives_packet(packet).map(Some),
        _ => Ok(None),
    }?;
    Ok(row.filter(|row| (1..=MAX_PLAUSIBLE_QUEST_ID).contains(&row.quest_id)))
}

fn parse_agent_spawned_packet(packet: &QuestPacketRow) -> Result<(u32, Option<u32>)> {
    let bytes = quest_packet_bytes(packet, 0x20, 0x74)?;
    let agent_id = u32_at(&bytes, 4).context("AGENT_SPAWNED agent_id is truncated")?;
    let agent_type = u32_at(&bytes, 8).context("AGENT_SPAWNED agent_type is truncated")?;
    let npc_model_id =
        (agent_type & 0xf000_0000 == 0x2000_0000).then_some(agent_type & 0x0fff_ffff);
    Ok((agent_id, npc_model_id))
}
fn parse_agent_despawned_packet(packet: &QuestPacketRow) -> Result<u32> {
    let bytes = quest_packet_bytes(packet, 0x21, 8)?;
    u32_at(&bytes, 4).context("AGENT_DESPAWNED agent_id is truncated")
}

fn parse_instance_load_info_packet(packet: &QuestPacketRow) -> Result<u32> {
    let bytes = quest_packet_bytes(packet, 0x199, 0x1c)?;
    u32_at(&bytes, 8).context("INSTANCE_LOAD_INFO map_id is truncated")
}

fn parse_npc_update_properties_packet(packet: &QuestPacketRow) -> Result<(u32, Option<Vec<u16>>)> {
    let bytes = quest_packet_bytes(packet, 0x56, 0x34)?;
    let npc_model_id = u32_at(&bytes, 4).context("NPC_UPDATE_PROPERTIES npc_id is truncated")?;
    Ok((npc_model_id, fixed_utf16_words(&bytes, 36, 8)))
}

fn parse_dialog_sender_packet(packet: &QuestPacketRow) -> Result<u32> {
    let bytes = quest_packet_bytes(packet, 0x81, 8)?;
    u32_at(&bytes, 4).context("DIALOG_SENDER agent_id is truncated")
}

fn parse_dialog_button_packet(packet: &QuestPacketRow) -> Result<Option<(u32, &'static str)>> {
    let bytes = quest_packet_bytes(packet, 0x7e, 0x110)?;
    let dialog_id = u32_at(&bytes, 264).context("DIALOG_BUTTON dialog_id is truncated")?;
    if dialog_id & 0x0080_0000 == 0 {
        return Ok(None);
    }
    let quest_id = (dialog_id ^ 0x0080_0000) >> 8;
    if !(1..=MAX_PLAUSIBLE_QUEST_ID).contains(&quest_id) {
        return Ok(None);
    }
    let dialog_type = match dialog_id & 0x0000_000f {
        1 => "take",
        2 => "decline",
        3 => "enquire",
        4 => "enquire_next",
        5 => "recap",
        6 => "enquire_reward",
        7 => "reward",
        _ => "unknown",
    };
    Ok(Some((quest_id, dialog_type)))
}
fn parse_quest_remove_id(packet: &QuestPacketRow) -> Result<u32> {
    let bytes = quest_packet_bytes(packet, 0x52, 8)?;
    quest_id(&bytes)
}

fn parse_quest_add_packet(packet: &QuestPacketRow) -> Result<QuestSnapshotRow> {
    let bytes = quest_packet_bytes(packet, 0x49, 0x50)?;
    let mut row = partial_quest_row(quest_id(&bytes)?);
    row.map_from = u32_at(&bytes, 76);
    row.location_encoded = fixed_utf16_words(&bytes, 28, 8);
    row.name_encoded = fixed_utf16_words(&bytes, 44, 8);
    row.npc_encoded = fixed_utf16_words(&bytes, 60, 8);
    Ok(row)
}

fn parse_quest_description_packet(packet: &QuestPacketRow) -> Result<QuestSnapshotRow> {
    let bytes = quest_packet_bytes(packet, 0x4c, 0x208)?;
    let mut row = partial_quest_row(quest_id(&bytes)?);
    row.description_encoded = fixed_utf16_words(&bytes, 8, 128);
    row.objectives_encoded = fixed_utf16_words(&bytes, 264, 128);
    Ok(row)
}

fn parse_quest_general_info_packet(packet: &QuestPacketRow) -> Result<QuestSnapshotRow> {
    let bytes = quest_packet_bytes(packet, 0x50, 0x40)?;
    let mut row = partial_quest_row(quest_id(&bytes)?);
    row.location_encoded = fixed_utf16_words(&bytes, 12, 8);
    row.name_encoded = fixed_utf16_words(&bytes, 28, 8);
    row.npc_encoded = fixed_utf16_words(&bytes, 44, 8);
    row.map_from = u32_at(&bytes, 60);
    Ok(row)
}

fn parse_quest_update_objectives_packet(packet: &QuestPacketRow) -> Result<QuestSnapshotRow> {
    let bytes = quest_packet_bytes(packet, 0x54, 0x108)?;
    let mut row = partial_quest_row(quest_id(&bytes)?);
    row.objectives_encoded = fixed_utf16_words(&bytes, 8, 128);
    Ok(row)
}

fn quest_packet_bytes(
    packet: &QuestPacketRow,
    expected_header: u32,
    expected_size: usize,
) -> Result<Vec<u8>> {
    if packet.header != expected_header {
        bail!(
            "packet row header 0x{:04X} does not match 0x{expected_header:04X}",
            packet.header
        );
    }
    let bytes = hex_to_bytes(&packet.raw_hex).context("quest packet raw_hex is not valid hex")?;
    if bytes.len() != expected_size {
        bail!(
            "quest packet 0x{expected_header:04X} has {} bytes instead of {expected_size}",
            bytes.len()
        );
    }
    if u32_at(&bytes, 0) != Some(expected_header) {
        bail!("raw quest packet header is not 0x{expected_header:04X}");
    }
    Ok(bytes)
}

fn quest_id(bytes: &[u8]) -> Result<u32> {
    u32_at(bytes, 4).context("quest packet quest_id is truncated")
}

fn partial_quest_row(quest_id: u32) -> QuestSnapshotRow {
    QuestSnapshotRow {
        quest_id,
        map_from: None,
        location_encoded: None,
        name_encoded: None,
        npc_encoded: None,
        description_encoded: None,
        objectives_encoded: None,
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn fixed_utf16_words(bytes: &[u8], offset: usize, capacity: usize) -> Option<Vec<u16>> {
    let mut words = Vec::with_capacity(capacity);
    for index in 0..capacity {
        let start = offset.checked_add(index.checked_mul(2)?)?;
        let word = u16::from_le_bytes(bytes.get(start..start.checked_add(2)?)?.try_into().ok()?);
        if word == 0 {
            break;
        }
        words.push(word);
    }
    (!words.is_empty()).then_some(words)
}

fn collect_text_inputs<'a>(
    quests: impl Iterator<Item = &'a QuestAccumulator>,
) -> (BTreeSet<u32>, BTreeMap<u32, u64>) {
    let mut ids = BTreeSet::new();
    let mut seeds = BTreeMap::new();
    for quest in quests {
        for words in [
            quest.encoded.location.as_deref(),
            quest.encoded.name.as_deref(),
            quest.encoded.npc.as_deref(),
            quest.encoded.description.as_deref(),
            quest.encoded.objectives.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            collect_text_input(words, &mut ids, &mut seeds);
        }
        for dialog in quest.dialogs.keys() {
            if let Some(words) = dialog.npc_encoded.as_deref() {
                collect_text_input(words, &mut ids, &mut seeds);
            }
        }
    }
    (ids, seeds)
}

fn collect_text_input(words: &[u16], ids: &mut BTreeSet<u32>, seeds: &mut BTreeMap<u32, u64>) {
    for text_ref in text_references(words) {
        ids.insert(text_ref.id);
        seeds.entry(text_ref.id).or_insert(text_ref.seed);
    }
    for segment in words.split(|word| *word == 0x0002) {
        let refs = text_references(segment);
        let Some(kind) = refs.first() else {
            continue;
        };
        if kind.id == 10_738
            && let Some(text_id) = tagged_reward_number(segment, kind.end, 0x010a)
        {
            ids.insert(text_id);
            seeds.entry(text_id).or_insert(0);
        }
    }
}

fn build_catalog_entry(
    quest_id: u32,
    quest: QuestAccumulator,
    lookup: &RuntimeTextLookup,
    reward_item_models: &RewardItemModelLookup,
) -> QuestCatalogEntry {
    let mut localized = BTreeMap::new();
    append_localized_field(
        &mut localized,
        "location",
        quest.encoded.location.as_deref(),
        lookup,
    );
    append_localized_field(
        &mut localized,
        "name",
        quest.encoded.name.as_deref(),
        lookup,
    );
    append_localized_field(&mut localized, "npc", quest.encoded.npc.as_deref(), lookup);
    append_localized_field(
        &mut localized,
        "description",
        quest.encoded.description.as_deref(),
        lookup,
    );
    let rewards = quest_rewards(
        quest.encoded.description.as_deref(),
        lookup,
        reward_item_models,
        &quest.reward_completions,
    );

    let observed_steps = quest
        .steps
        .into_iter()
        .map(|step| catalog_step(step, lookup))
        .collect();
    let observed_step_sequences = quest
        .step_sequences
        .into_values()
        .map(|sequence| {
            sequence
                .into_iter()
                .map(|step| catalog_step(step, lookup))
                .collect()
        })
        .collect();

    let dialogs = quest
        .dialogs
        .into_iter()
        .map(|(key, dialog)| {
            let mut localized = BTreeMap::new();
            append_localized_field(&mut localized, "npc", key.npc_encoded.as_deref(), lookup);
            QuestDialogEvidence {
                role: key.role,
                npc_model_id: key.npc_model_id,
                map_ids: dialog.map_ids.into_iter().collect(),
                localized,
            }
        })
        .collect();

    QuestCatalogEntry {
        quest_id,
        origin_map_ids: quest.origin_map_ids.into_iter().collect(),
        localized,
        observed_steps,
        observed_step_sequences,
        rewards,
        dialogs,
    }
}

fn catalog_step(step: StepObservation, lookup: &RuntimeTextLookup) -> QuestStep {
    let mut localized = BTreeMap::new();
    append_localized_step(&mut localized, "text", &step.encoded_words, lookup);
    QuestStep { localized }
}

fn dialog_role(dialog_type: &str) -> Option<&'static str> {
    match dialog_type {
        "take" | "enquire" => Some("giver"),
        "enquire_reward" | "reward" => Some("reward"),
        "enquire_next" | "recap" => Some("progress"),
        _ => None,
    }
}

fn append_localized_field(
    out: &mut BTreeMap<String, String>,
    prefix: &str,
    encoded_words: Option<&[u16]>,
    lookup: &RuntimeTextLookup,
) {
    let Some(words) = encoded_words else {
        return;
    };
    let refs = text_references(words);
    append_localized_references(out, prefix, &refs, lookup);
}

fn append_localized_step(
    out: &mut BTreeMap<String, String>,
    prefix: &str,
    encoded_words: &[u16],
    lookup: &RuntimeTextLookup,
) {
    let refs = text_references(encoded_words);
    let refs = if refs
        .first()
        .is_some_and(|text_ref| matches!(text_ref.id, 10_741 | 10_742))
    {
        &refs[1..]
    } else {
        &refs
    };
    append_localized_references(out, prefix, refs, lookup);
}

fn append_localized_references(
    out: &mut BTreeMap<String, String>,
    prefix: &str,
    refs: &[TextReference],
    lookup: &RuntimeTextLookup,
) {
    let Some(template) = refs.first() else {
        return;
    };
    let Some(template_names) = lookup.by_text_id.get(&template.id) else {
        return;
    };
    for (code, template_text) in template_names {
        let args = refs[1..]
            .iter()
            .map(|text_ref| {
                lookup
                    .by_text_id
                    .get(&text_ref.id)
                    .and_then(|texts| texts.get(code).or_else(|| texts.get("en")))
                    .cloned()
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        let text = apply_encoded_template(template_text, &args)
            .replace("{sc}", "")
            .replace("{s}", "")
            .trim_matches('%')
            .trim()
            .to_string();
        if !text.is_empty() {
            out.insert(format!("{prefix}_{code}"), text);
        }
    }
}

fn quest_rewards(
    encoded_words: Option<&[u16]>,
    lookup: &RuntimeTextLookup,
    item_models: &RewardItemModelLookup,
    reward_completions: &BTreeSet<CapturePoint>,
) -> QuestRewards {
    let Some(words) = encoded_words else {
        return QuestRewards::default();
    };
    let mut rewards = QuestRewards::default();
    for segment in words.split(|word| *word == 0x0002) {
        let refs = text_references(segment);
        let Some(kind) = refs.first() else {
            continue;
        };
        match kind.id {
            10_730 => rewards.experience = tagged_reward_number(segment, kind.end, 0x0101),
            10_732 => rewards.gold = tagged_reward_number(segment, kind.end, 0x0101),
            10_735 => {
                let Some(name) = refs.get(1) else {
                    continue;
                };
                let model =
                    reward_item_model((name.id, name.seed), reward_completions, item_models);
                rewards.items.push(QuestRewardItem {
                    model_id: model.map(|model| model.model_id),
                    model_file_id: model.and_then(|model| model.model_file_id),
                    localized: localized_reference("name", name.id, lookup),
                });
            }
            10_738 => {
                let Some(name_text_id) = tagged_reward_number(segment, kind.end, 0x010a) else {
                    continue;
                };
                rewards.skills.push(QuestRewardSkill {
                    localized: localized_reference("name", name_text_id, lookup),
                });
            }
            _ => {}
        }
    }
    rewards
}
fn reward_item_model(
    text_reference: (u32, u64),
    reward_completions: &BTreeSet<CapturePoint>,
    item_models: &RewardItemModelLookup,
) -> Option<RewardItemModel> {
    let temporal = item_models
        .observations
        .get(&text_reference)
        .into_iter()
        .flatten()
        .filter(|(item, _)| {
            reward_completions.iter().any(|completion| {
                item.session_id == completion.session_id
                    && item.ts_ms.abs_diff(completion.ts_ms) <= REWARD_ITEM_CORRELATION_WINDOW_MS
            })
        })
        .map(|(_, model)| *model)
        .collect::<BTreeSet<_>>();
    if temporal.len() == 1 {
        temporal.first().copied()
    } else {
        item_models.unique.get(&text_reference).copied()
    }
}

fn tagged_reward_number(words: &[u16], start: usize, expected_tag: u16) -> Option<u32> {
    let tag = words
        .get(start..)?
        .iter()
        .position(|word| *word == expected_tag)?
        + start;
    let value = encoded_values_from_words(words.get(tag + 1..)?)?.first()?.0;
    u32::try_from(value).ok()
}

fn localized_reference(
    prefix: &str,
    text_id: u32,
    lookup: &RuntimeTextLookup,
) -> BTreeMap<String, String> {
    lookup
        .by_text_id
        .get(&text_id)
        .into_iter()
        .flat_map(|texts| texts.iter())
        .map(|(code, text)| (format!("{prefix}_{code}"), text.clone()))
        .collect()
}

fn objective_steps(words: &[u16]) -> Vec<StepObservation> {
    words
        .split(|word| *word == 0x0002)
        .filter_map(|segment| {
            if segment.is_empty() {
                return None;
            }
            let content_start = segment
                .iter()
                .position(|word| *word == 0x010a)
                .map_or(0, |index| index + 1);
            let text_ref = text_references(&segment[content_start..])
                .into_iter()
                .next()?;
            Some(StepObservation {
                text_ref,
                encoded_words: segment.to_vec(),
            })
        })
        .collect()
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
mod tests {
    use super::*;

    const BLOOD_OBJECTIVE: &str = "f62ab7c12dcf21130a0102815526318410c5314f0100";

    #[test]
    fn parses_content_reference() {
        let steps = objective_steps(&encoded_words_from_hex(BLOOD_OBJECTIVE).unwrap());

        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].text_ref.id, 74_581);
        assert_eq!(steps[0].text_ref.seed, 864_160_136_753);
    }

    #[test]
    fn decodes_legacy_snapshot_hex_once_at_input_boundary() {
        let row: QuestSnapshotRow = serde_json::from_value(serde_json::json!({
            "quest_id": 0x389,
            "location_enc_hex": "0181db765be28fc50f2a"
        }))
        .unwrap();
        assert_eq!(
            row.location_encoded.as_deref().map(words_to_hex).as_deref(),
            Some("0181db765be28fc50f2a")
        );
    }

    #[test]
    fn resolves_localized_quest_field_from_text_lookup() {
        let mut lookup = RuntimeTextLookup::default();
        lookup.by_text_id.insert(
            62_683,
            BTreeMap::from([
                ("en".to_string(), "Norn".to_string()),
                ("fr".to_string(), "Norn".to_string()),
            ]),
        );
        let mut out = BTreeMap::new();

        let words = encoded_words_from_hex("0181db765be28fc50f2a").unwrap();
        append_localized_field(&mut out, "location", Some(&words), &lookup);

        assert_eq!(out["location_en"], "Norn");
        assert_eq!(out["location_fr"], "Norn");
    }

    #[test]
    fn catalog_json_contains_only_consumer_fields() {
        let entry = QuestCatalogEntry {
            quest_id: 0x389,
            origin_map_ids: vec![642],
            localized: BTreeMap::from([("name_en".to_string(), "A Gate Too Far".to_string())]),
            observed_steps: vec![QuestStep {
                localized: BTreeMap::from([("text_en".to_string(), "Talk to Rurik.".to_string())]),
            }],
            observed_step_sequences: Vec::new(),
            rewards: QuestRewards {
                experience: Some(100),
                gold: None,
                items: vec![QuestRewardItem {
                    model_id: Some(2_817),
                    model_file_id: Some(9_528),
                    localized: BTreeMap::from([("name_en".to_string(), "Long Sword".to_string())]),
                }],
                skills: Vec::new(),
            },
            dialogs: vec![QuestDialogEvidence {
                role: "giver",
                npc_model_id: Some(1459),
                map_ids: vec![146],
                localized: BTreeMap::from([("npc_en".to_string(), "Prince Rurik".to_string())]),
            }],
        };

        let value = serde_json::to_value(vec![entry]).unwrap();
        let quest = &value[0];

        assert!(value.is_array());
        assert_eq!(quest["quest_id"], 0x389);
        assert_eq!(quest["name_en"], "A Gate Too Far");
        assert_eq!(quest["origin_map_ids"], serde_json::json!([642]));
        for field in [
            "last_observed_ts_ms",
            "log_state",
            "flags",
            "h0024",
            "observed_removed",
            "encoded",
            "map_from",
            "map_to",
            "marker",
        ] {
            assert!(
                quest.get(field).is_none(),
                "{field} leaked into quests.json"
            );
        }
        for field in [
            "text_id",
            "text_seed",
            "observed_completed",
            "observed_pending",
            "encoded_hex",
        ] {
            assert!(quest["observed_steps"][0].get(field).is_none());
        }
        for field in ["name_text_id", "name_text_seed", "encoded_hex"] {
            assert!(quest["rewards"]["items"][0].get(field).is_none());
        }
        assert_eq!(quest["rewards"]["items"][0]["model_id"], 2_817);
        assert_eq!(quest["rewards"]["items"][0]["model_file_id"], 9_528);
        assert!(quest["quest_npcs"][0].get("agent_id").is_none());
        assert!(quest["quest_npcs"][0].get("encoded").is_none());
    }

    #[test]
    fn resolves_composed_reward_return_step() {
        let encoded = "f62ab7c12dcf21130a01f02aa7c4009cbb3e0a018033b08113f2d45e01000100";
        let words = encoded_words_from_hex(encoded).unwrap();
        let refs = text_references(&words);
        assert_eq!(refs.len(), 3);
        let mut lookup = RuntimeTextLookup::default();
        lookup.by_text_id.insert(
            refs[1].id,
            BTreeMap::from([(
                "fr".to_string(),
                "Pour votre récompense retournez voir : %str1%".to_string(),
            )]),
        );
        lookup.by_text_id.insert(
            refs[2].id,
            BTreeMap::from([("fr".to_string(), "Prince Rurik".to_string())]),
        );
        let mut out = BTreeMap::new();

        append_localized_step(&mut out, "text", &words, &lookup);

        assert_eq!(
            out["text_fr"],
            "Pour votre récompense retournez voir : Prince Rurik"
        );
    }

    #[test]
    fn extracts_structured_rewards_from_description() {
        let encoded = "b51558dc67c2157302000201020002010200e82ad4e7cce572360200ea2a3f8c19b511660101c8010200ec2ac7daae818234010132010200ef2a90f66ed0534c0a010d2728eb10d1d97701000b01890a0a014e0a01000b01e008010001010601020109010100";
        let encoded_words = encoded_words_from_hex(encoded).unwrap();
        let mut lookup = RuntimeTextLookup::default();
        lookup.by_text_id.insert(
            9_741,
            BTreeMap::from([
                ("en".to_string(), "Long Sword".to_string()),
                ("fr".to_string(), "Epée longue".to_string()),
            ]),
        );
        lookup.by_text_id.insert(
            24_964,
            BTreeMap::from([("fr".to_string(), "Sceau de résurrection".to_string())]),
        );

        let rewards = quest_rewards(
            Some(&encoded_words),
            &lookup,
            &RewardItemModelLookup::default(),
            &BTreeSet::new(),
        );

        assert_eq!(rewards.experience, Some(200));
        assert_eq!(rewards.gold, Some(50));
        assert_eq!(rewards.items.len(), 1);
        assert_eq!(rewards.items[0].localized["name_en"], "Long Sword");
        assert_eq!(rewards.items[0].localized["name_fr"], "Epée longue");

        let skill_words = encoded_words_from_hex("f22aa7c7db8411490a0184620100").unwrap();
        let skill_rewards = quest_rewards(
            Some(&skill_words),
            &lookup,
            &RewardItemModelLookup::default(),
            &BTreeSet::new(),
        );
        assert_eq!(skill_rewards.skills.len(), 1);
        assert_eq!(
            skill_rewards.skills[0].localized["name_fr"],
            "Sceau de résurrection"
        );
        let mut row = partial_quest_row(0x38);
        row.description_encoded = encoded_words_from_hex("f22aa7c7db8411490a0184620100");
        let mut quest = QuestAccumulator::new(&row);
        quest.observe(row);
        let (ids, seeds) = collect_text_inputs(std::iter::once(&quest));
        assert!(ids.contains(&24_964));
        assert_eq!(seeds[&24_964], 0);
    }

    #[test]
    fn maps_only_exact_unique_reward_item_references_to_models() {
        const REWARD: &str =
            "ef2a90f66ed0534c0a0151262984a6d6373201000b01860a0a01440a0100010104010100";
        let reward_words = encoded_words_from_hex(REWARD).unwrap();
        const ITEM: &str = r#"{"model_id":2817,"model_file_id":9528,"name_text_id":9553,"enc_name_hex":"51262984a6d637320000"}"#;
        let path = std::env::temp_dir().join(format!(
            "tyria_reward_item_models_{}.jsonl",
            std::process::id()
        ));
        std::fs::write(&path, ITEM).unwrap();

        let item_models = read_reward_item_models(&path).unwrap();
        let rewards = quest_rewards(
            Some(&reward_words),
            &RuntimeTextLookup::default(),
            &item_models,
            &BTreeSet::new(),
        );
        assert_eq!(rewards.items[0].model_id, Some(2_817));
        assert_eq!(rewards.items[0].model_file_id, Some(9_528));

        std::fs::write(
            &path,
            format!(
                "{ITEM}\n{{\"model_id\":9999,\"model_file_id\":8888,\"name_text_id\":9553,\"enc_name_hex\":\"51262984a6d637320000\"}}\n"
            ),
        )
        .unwrap();
        let ambiguous_models = read_reward_item_models(&path).unwrap();
        let ambiguous_reward = quest_rewards(
            Some(&reward_words),
            &RuntimeTextLookup::default(),
            &ambiguous_models,
            &BTreeSet::new(),
        );
        assert_eq!(ambiguous_reward.items[0].model_id, None);
        assert_eq!(ambiguous_reward.items[0].model_file_id, None);
        std::fs::write(
            &path,
            concat!(
                "{\"ts_ms\":1100,\"session_id\":7,\"model_id\":2817,\"model_file_id\":9528,",
                "\"name_text_id\":9553,\"enc_name_hex\":\"51262984a6d637320000\"}\n",
                "{\"ts_ms\":1100,\"session_id\":8,\"model_id\":9999,\"model_file_id\":8888,",
                "\"name_text_id\":9553,\"enc_name_hex\":\"51262984a6d637320000\"}\n"
            ),
        )
        .unwrap();
        let timed_models = read_reward_item_models(&path).unwrap();
        let offers = BTreeSet::from([CapturePoint {
            session_id: 7,
            ts_ms: 1_000,
        }]);
        let correlated_reward = quest_rewards(
            Some(&reward_words),
            &RuntimeTextLookup::default(),
            &timed_models,
            &offers,
        );
        assert_eq!(correlated_reward.items[0].model_id, Some(2_817));
        assert_eq!(correlated_reward.items[0].model_file_id, Some(9_528));

        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn preserves_origin_conflicts_and_observed_step_sequences() {
        let mut first = partial_quest_row(0x389);
        first.map_from = Some(146);
        first.objectives_encoded = encoded_words_from_hex(BLOOD_OBJECTIVE);
        let mut quest = QuestAccumulator::new(&first);
        quest.observe(first);

        let mut second = partial_quest_row(0x389);
        second.map_from = Some(148);
        second.objectives_encoded =
            encoded_words_from_hex("f62ab7c12dcf21130a010181c674a0f1b184256e0100");
        quest.observe(second);

        assert_eq!(quest.origin_map_ids, BTreeSet::from([146, 148]));
        assert_eq!(quest.steps.len(), 2);
        assert_eq!(quest.step_sequences.len(), 2);
    }

    #[test]
    fn decodes_quest_add_from_direct_client_packet() {
        let mut bytes = [0_u8; 0x50];
        let put_u32 = |bytes: &mut [u8], offset: usize, value: u32| {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        };
        put_u32(&mut bytes, 0, 0x49);
        put_u32(&mut bytes, 4, 0x389);
        put_u32(&mut bytes, 8, 12.5_f32.to_bits());
        put_u32(&mut bytes, 12, (-8.0_f32).to_bits());
        put_u32(&mut bytes, 16, 3);
        put_u32(&mut bytes, 20, 482);
        put_u32(&mut bytes, 24, 0x4d);
        for (index, word) in [0x8101_u16, 0x76db, 0xe25b, 0xc58f, 0x2a0f]
            .into_iter()
            .enumerate()
        {
            bytes[28 + index * 2..30 + index * 2].copy_from_slice(&word.to_le_bytes());
        }
        put_u32(&mut bytes, 76, 642);
        let raw_hex = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        let packet = QuestPacketRow {
            ts_ms: 99,
            session_id: 7,
            header: 0x49,
            raw_hex,
        };
        let row = parse_quest_add_packet(&packet).unwrap();

        assert_eq!(row.quest_id, 0x389);
        assert_eq!(row.map_from, Some(642));
        assert_eq!(
            row.location_encoded.as_deref().map(words_to_hex).as_deref(),
            Some("0181db765be28fc50f2a")
        );
    }

    #[test]
    fn links_quest_dialog_to_stable_npc_model() {
        let put_u32 = |bytes: &mut [u8], offset: usize, value: u32| {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        };
        let put_words = |bytes: &mut [u8], offset: usize, words: &[u16]| {
            for (index, word) in words.iter().enumerate() {
                let start = offset + index * 2;
                bytes[start..start + 2].copy_from_slice(&word.to_le_bytes());
            }
        };
        let packet = |ts_ms: u128, header: u32, bytes: &[u8]| {
            serde_json::json!({
                "ts_ms": ts_ms,
                "kind": "quest_packet",
                "header": header,
                "raw_hex": bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
            })
        };

        let mut map = [0_u8; 0x1c];
        put_u32(&mut map, 0, 0x199);
        put_u32(&mut map, 8, 146);
        let mut npc = [0_u8; 0x34];
        put_u32(&mut npc, 0, 0x56);
        put_u32(&mut npc, 4, 0x5b3);
        put_words(&mut npc, 36, &[0x8101, 0x76db, 0xe25b]);
        let mut spawned = [0_u8; 0x74];
        put_u32(&mut spawned, 0, 0x20);
        put_u32(&mut spawned, 4, 41);
        put_u32(&mut spawned, 8, 0x2000_05b3);
        let mut sender = [0_u8; 8];
        put_u32(&mut sender, 0, 0x81);
        put_u32(&mut sender, 4, 41);
        let mut button = [0_u8; 0x110];
        put_u32(&mut button, 0, 0x7e);
        put_words(&mut button, 8, &[0x8102, 0x1978, 0x010a]);
        put_u32(&mut button, 264, (0x389 << 8) | 0x0080_0003);
        let mut decline_button = button;
        put_u32(&mut decline_button, 264, (0x389 << 8) | 0x0080_0002);
        let mut reward_button = button;
        put_u32(&mut reward_button, 264, (0x389 << 8) | 0x0080_0007);
        let mut remove = [0_u8; 8];
        put_u32(&mut remove, 0, 0x52);
        put_u32(&mut remove, 4, 0x389);

        let rows = [
            serde_json::json!({
                "kind": "quest_status",
                "status": "quest_hooks_installed"
            }),
            packet(1, 0x199, &map),
            packet(2, 0x56, &npc),
            packet(3, 0x20, &spawned),
            packet(4, 0x81, &sender),
            packet(5, 0x7e, &button),
            packet(6, 0x7e, &decline_button),
            packet(7, 0x7e, &reward_button),
            packet(8, 0x52, &remove),
        ];
        let path = std::env::temp_dir().join(format!(
            "tyria-extractor-quest-dialog-{}.jsonl",
            std::process::id()
        ));
        std::fs::write(
            &path,
            rows.into_iter()
                .map(|row| row.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();

        let quests = read_quest_accumulators(&path).unwrap();
        std::fs::remove_file(path).unwrap();

        let quest = &quests[&0x389];
        assert_eq!(quest.dialogs.len(), 2);
        let (dialog, evidence) = quest
            .dialogs
            .iter()
            .find(|(dialog, _)| dialog.role == "giver")
            .unwrap();
        assert_eq!(dialog.role, "giver");
        assert_eq!(dialog.agent_id, None);
        assert_eq!(dialog.npc_model_id, Some(0x5b3));
        assert_eq!(
            dialog.npc_encoded.as_deref().map(words_to_hex).as_deref(),
            Some("0181db765be2")
        );
        assert_eq!(evidence.map_ids, BTreeSet::from([146]));
        assert_eq!(
            quest.reward_completions,
            BTreeSet::from([CapturePoint {
                session_id: 0,
                ts_ms: 8,
            }])
        );
        let decline = parse_dialog_button_packet(&QuestPacketRow {
            ts_ms: 5,
            session_id: 0,
            header: 0x7e,
            raw_hex: decline_button
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        })
        .unwrap()
        .unwrap();
        assert_eq!(decline.0, 0x389);
        assert_eq!(decline.1, "decline");
        assert_eq!(dialog_role(decline.1), None);
        let mut high_quest_button = button;
        put_u32(&mut high_quest_button, 264, (0x1389 << 8) | 0x0080_0003);
        let high_quest = parse_dialog_button_packet(&QuestPacketRow {
            ts_ms: 6,
            session_id: 0,
            header: 0x7e,
            raw_hex: high_quest_button
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        })
        .unwrap()
        .unwrap();
        assert_eq!(high_quest, (0x1389, "enquire"));
    }

    #[test]
    fn decodes_consumer_relevant_quest_packet_families() {
        let packet = |header: u32, bytes: &[u8]| QuestPacketRow {
            ts_ms: 123,
            session_id: 0,
            header,
            raw_hex: bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
        };
        let put_u32 = |bytes: &mut [u8], offset: usize, value: u32| {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        };
        let put_words = |bytes: &mut [u8], offset: usize, words: &[u16]| {
            for (index, word) in words.iter().enumerate() {
                let start = offset + index * 2;
                bytes[start..start + 2].copy_from_slice(&word.to_le_bytes());
            }
        };

        let mut general = [0_u8; 0x40];
        put_u32(&mut general, 0, 0x50);
        put_u32(&mut general, 4, 0x389);
        put_u32(&mut general, 8, 0x4c);
        put_words(&mut general, 12, &[0x8101, 0x76db, 0xe25b, 0xc58f, 0x2a0f]);
        put_u32(&mut general, 60, 499);
        let general = parse_quest_packet(&packet(0x50, &general))
            .unwrap()
            .unwrap();
        assert_eq!(general.map_from, Some(499));
        assert_eq!(
            general
                .location_encoded
                .as_deref()
                .map(words_to_hex)
                .as_deref(),
            Some("0181db765be28fc50f2a")
        );

        let mut description = [0_u8; 0x208];
        put_u32(&mut description, 0, 0x4c);
        put_u32(&mut description, 4, 0x389);
        put_words(&mut description, 8, &[0x8102, 0x1978, 0x010a]);
        put_words(&mut description, 264, &[0x2af6, 0xc1b7, 0xcf2d, 0x1321]);
        let description = parse_quest_packet(&packet(0x4c, &description))
            .unwrap()
            .unwrap();
        assert_eq!(
            description
                .description_encoded
                .as_deref()
                .map(words_to_hex)
                .as_deref(),
            Some("028178190a01")
        );
        assert_eq!(
            description
                .objectives_encoded
                .as_deref()
                .map(words_to_hex)
                .as_deref(),
            Some("f62ab7c12dcf2113")
        );

        let mut marker = [0_u8; 0x18];
        put_u32(&mut marker, 0, 0x51);
        put_u32(&mut marker, 4, 0x389);
        put_u32(&mut marker, 8, 1.5_f32.to_bits());
        put_u32(&mut marker, 12, 2.5_f32.to_bits());
        put_u32(&mut marker, 16, 4);
        put_u32(&mut marker, 20, 546);
        assert!(
            parse_quest_packet(&packet(0x51, &marker))
                .unwrap()
                .is_none()
        );

        let mut objectives = [0_u8; 0x108];
        put_u32(&mut objectives, 0, 0x54);
        put_u32(&mut objectives, 4, 0x389);
        put_words(&mut objectives, 8, &[0x8101, 0x74c6]);
        let objectives = parse_quest_packet(&packet(0x54, &objectives))
            .unwrap()
            .unwrap();
        assert_eq!(
            objectives
                .objectives_encoded
                .as_deref()
                .map(words_to_hex)
                .as_deref(),
            Some("0181c674")
        );

        let mut bogus_objectives = [0_u8; 0x108];
        put_u32(&mut bogus_objectives, 0, 0x54);
        put_u32(&mut bogus_objectives, 4, 0x532c_c66c);
        assert!(
            parse_quest_packet(&packet(0x54, &bogus_objectives))
                .unwrap()
                .is_none()
        );

        let mut remove = [0_u8; 8];
        put_u32(&mut remove, 0, 0x52);
        put_u32(&mut remove, 4, 0x389);
        assert!(
            parse_quest_packet(&packet(0x52, &remove))
                .unwrap()
                .is_none()
        );
    }
}
