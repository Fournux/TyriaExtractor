use super::*;

const REWARD_DIALOG_LIFETIME_MS: u128 = 30 * 60 * 1_000;

pub(super) fn read_reward_item_models(path: &Path) -> Result<RewardItemModelLookup> {
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

pub(super) fn read_quest_accumulators(path: &Path) -> Result<BTreeMap<u32, QuestAccumulator>> {
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
