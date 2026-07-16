use super::*;

const REWARD_ITEM_CORRELATION_WINDOW_MS: u128 = 120_000;

pub(super) fn collect_text_inputs<'a>(
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
        for step in &quest.steps {
            collect_text_input(&step.encoded_words, &mut ids, &mut seeds);
        }
        for dialog in quest.dialogs.keys() {
            if let Some(words) = dialog.npc_encoded.as_deref() {
                collect_text_input(words, &mut ids, &mut seeds);
            }
        }
    }
    (ids, seeds)
}

pub(super) fn collect_text_input(
    words: &[u16],
    ids: &mut BTreeSet<u32>,
    seeds: &mut BTreeMap<u32, u64>,
) {
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

pub(super) fn build_catalog_entry(
    quest_id: u32,
    quest: QuestAccumulator,
    lookup: &LocalizedTextCatalog,
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
        .into_iter()
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

pub(super) fn catalog_step(step: StepObservation, lookup: &LocalizedTextCatalog) -> QuestStep {
    let mut localized = BTreeMap::new();
    append_localized_step(&mut localized, "text", &step.encoded_words, lookup);
    QuestStep { localized }
}

pub(super) fn dialog_role(dialog_type: &str) -> Option<&'static str> {
    match dialog_type {
        "take" | "enquire" => Some("giver"),
        "enquire_reward" | "reward" => Some("reward"),
        "enquire_next" | "recap" => Some("progress"),
        _ => None,
    }
}

pub(super) fn append_localized_field(
    out: &mut BTreeMap<String, String>,
    prefix: &str,
    encoded_words: Option<&[u16]>,
    lookup: &LocalizedTextCatalog,
) {
    let Some(words) = encoded_words else {
        return;
    };
    let refs = text_references(words);
    append_localized_references(out, prefix, &refs, lookup);
}

pub(super) fn append_localized_step(
    out: &mut BTreeMap<String, String>,
    prefix: &str,
    encoded_words: &[u16],
    lookup: &LocalizedTextCatalog,
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

pub(super) fn append_localized_references(
    out: &mut BTreeMap<String, String>,
    prefix: &str,
    refs: &[TextReference],
    lookup: &LocalizedTextCatalog,
) {
    for_each_localized_reference(refs, lookup, |code, text| {
        out.insert(format!("{prefix}_{code}"), text.to_string());
    });
}

pub(super) fn quest_rewards(
    encoded_words: Option<&[u16]>,
    lookup: &LocalizedTextCatalog,
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
pub(super) fn reward_item_model(
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

pub(super) fn tagged_reward_number(words: &[u16], start: usize, expected_tag: u16) -> Option<u32> {
    let tag = words
        .get(start..)?
        .iter()
        .position(|word| *word == expected_tag)?
        + start;
    let value = encoded_values_from_words(words.get(tag + 1..)?)?.first()?.0;
    u32::try_from(value).ok()
}

pub(super) fn localized_reference(
    prefix: &str,
    text_id: u32,
    lookup: &LocalizedTextCatalog,
) -> BTreeMap<String, String> {
    lookup
        .by_text_id
        .get(&text_id)
        .into_iter()
        .flat_map(|texts| texts.iter())
        .map(|(code, text)| (format!("{prefix}_{code}"), text.clone()))
        .collect()
}

pub(super) fn objective_steps(words: &[u16]) -> Vec<StepObservation> {
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
