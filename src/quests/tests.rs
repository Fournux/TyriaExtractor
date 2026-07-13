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
    let mut lookup = LocalizedTextCatalog::default();
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
    let mut lookup = LocalizedTextCatalog::default();
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
    let mut lookup = LocalizedTextCatalog::default();
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
    const REWARD: &str = "ef2a90f66ed0534c0a0151262984a6d6373201000b01860a0a01440a0100010104010100";
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
        &LocalizedTextCatalog::default(),
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
        &LocalizedTextCatalog::default(),
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
        &LocalizedTextCatalog::default(),
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
fn collects_text_inputs_from_every_observed_step() {
    let first_words = encoded_words_from_hex(BLOOD_OBJECTIVE).unwrap();
    let second_words =
        encoded_words_from_hex("f62ab7c12dcf21130a010181c674a0f1b184256e0100").unwrap();
    let first_id = objective_steps(&first_words)[0].text_ref.id;
    let second_id = objective_steps(&second_words)[0].text_ref.id;
    assert_ne!(first_id, second_id);

    let mut first = partial_quest_row(0x389);
    first.objectives_encoded = Some(first_words);
    let mut quest = QuestAccumulator::new(&first);
    quest.observe(first);

    let mut second = partial_quest_row(0x389);
    second.objectives_encoded = Some(second_words);
    quest.observe(second);

    let (ids, _) = collect_text_inputs(std::iter::once(&quest));
    assert!(ids.contains(&first_id));
    assert!(ids.contains(&second_id));

    let mut lookup = LocalizedTextCatalog::default();
    lookup.by_text_id.insert(
        first_id,
        BTreeMap::from([("en".to_string(), "First observed step".to_string())]),
    );
    lookup.by_text_id.insert(
        second_id,
        BTreeMap::from([("en".to_string(), "Second observed step".to_string())]),
    );
    let entry = build_catalog_entry(0x389, quest, &lookup, &RewardItemModelLookup::default());
    let texts = entry
        .observed_steps
        .iter()
        .filter_map(|step| step.localized.get("text_en").map(String::as_str))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        texts,
        BTreeSet::from(["First observed step", "Second observed step"])
    );
}

#[test]
fn preserves_step_payload_variants_with_same_primary_reference() {
    let original = encoded_words_from_hex(BLOOD_OBJECTIVE).unwrap();
    let mut variant = original.clone();
    variant.push(0x0003);
    assert_eq!(
        objective_steps(&original)[0].text_ref,
        objective_steps(&variant)[0].text_ref
    );

    let mut first = partial_quest_row(0x389);
    first.objectives_encoded = Some(original);
    let mut quest = QuestAccumulator::new(&first);
    quest.observe(first);

    let mut second = partial_quest_row(0x389);
    second.objectives_encoded = Some(variant);
    quest.observe(second);

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
