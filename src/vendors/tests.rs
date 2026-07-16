use super::*;
use crate::tests::TestDir;

fn packet_row(header: u32, bytes: &[u8]) -> String {
    serde_json::json!({
        "kind": "world_packet",
        "session_id": 7,
        "header": header,
        "raw_hex": hex::encode(bytes),
    })
    .to_string()
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn agent_name_packet(agent_id: u32, words: &[u16]) -> [u8; 0x48] {
    assert!(words.len() <= 32);
    let mut packet = [0_u8; 0x48];
    put_u32(&mut packet, 0, 0x009b);
    put_u32(&mut packet, 4, agent_id);
    for (index, word) in words.iter().enumerate() {
        packet[8 + index * 2..10 + index * 2].copy_from_slice(&word.to_le_bytes());
    }
    packet
}

#[test]
fn writes_dedicated_vendor_catalogs() -> Result<()> {
    let mut load = [0_u8; 0x1c];
    put_u32(&mut load, 0, 0x0199);
    put_u32(&mut load, 8, 146);
    let mut spawn = [0_u8; 0x74];
    put_u32(&mut spawn, 0, 0x0020);
    put_u32(&mut spawn, 4, 42);
    put_u32(&mut spawn, 8, 0x2000_0000 | 525);
    put_u32(&mut spawn, 0x14, 10.0_f32.to_bits());
    put_u32(&mut spawn, 0x18, 20.0_f32.to_bits());
    let mut second_spawn = spawn;
    put_u32(&mut second_spawn, 4, 43);
    put_u32(&mut second_spawn, 0x14, 30.0_f32.to_bits());
    put_u32(&mut second_spawn, 0x18, 40.0_f32.to_bits());
    let brownlow_words = vec![0x8101, 0x8102];
    let brownlow_name = agent_name_packet(42, &brownlow_words);
    let jacob_words = vec![0x8201, 0x8202];
    let jacob_name = agent_name_packet(43, &jacob_words);
    let offer = serde_json::json!({
        "kind": "collector_offers",
        "session_id": 7,
        "merchant_agent_id": 42,
        "npc_model_id": null,
        "transaction_service": 2,
        "required_item": {"model_id": 948, "quantity": 4},
        "reward_count": 1,
        "captured_reward_count": 1,
        "rewards": [{"model_id": 12, "model_file_id": 202, "item_type": 2}],
    })
    .to_string();
    let second_offer = serde_json::json!({
        "kind": "collector_offers",
        "session_id": 7,
        "merchant_agent_id": 43,
        "npc_model_id": 525,
        "transaction_service": 2,
        "required_item": {"model_id": 949, "quantity": 3},
        "reward_count": 1,
        "captured_reward_count": 1,
        "rewards": [{"model_id": 15, "model_file_id": 205, "item_type": 4}],
    })
    .to_string();
    let merchant = serde_json::json!({
        "kind": "merchant_items",
        "session_id": 7,
        "merchant_agent_id": 42,
        "npc_model_id": 525,
        "transaction_service": 1,
        "entry_count": 1,
        "captured_entry_count": 1,
        "capture_complete": true,
        "entries": [{
            "item_id": 901,
            "model_id": 13,
            "model_file_id": 203,
            "item_type": 3,
            "base_value": 50
        }]
    })
    .to_string();
    let crafter = serde_json::json!({
        "kind": "crafter_products",
        "session_id": 7,
        "merchant_agent_id": 42,
        "npc_model_id": 525,
        "transaction_service": 3,
        "entry_count": 1,
        "captured_entry_count": 1,
        "capture_complete": true,
        "entries": [{
            "item_id": 902,
            "model_id": 14,
            "model_file_id": 204,
            "item_type": 4,
            "base_value": 60
        }]
    })
    .to_string();
    let trainer = serde_json::json!({
        "kind": "skill_trainer_skills",
        "session_id": 7,
        "merchant_agent_id": 42,
        "npc_model_id": 525,
        "transaction_service": 10,
        "entry_count": 1,
        "captured_entry_count": 1,
        "capture_complete": true,
        "entries": [{"skill_id": 900, "availability_flags_raw": 1}]
    })
    .to_string();
    let temp = TestDir::new()?;
    let input = temp.path().join("capture.jsonl");
    std::fs::write(
        &input,
        [
            packet_row(0x0199, &load),
            packet_row(0x0020, &spawn),
            offer.clone(),
            packet_row(0x009b, &brownlow_name),
            packet_row(0x0020, &spawn),
            offer,
            packet_row(0x009b, &jacob_name),
            packet_row(0x0020, &second_spawn),
            second_offer,
            merchant,
            crafter,
            trainer,
        ]
        .join("\n"),
    )?;

    assert_eq!(
        captured_npc_name_words(&input)?,
        BTreeSet::from([brownlow_words.clone(), jacob_words.clone()])
    );
    let localized_npc_names = BTreeMap::from([
        (
            brownlow_words,
            BTreeMap::from([("name_en".to_string(), "Brownlow [Collector]".to_string())]),
        ),
        (
            jacob_words,
            BTreeMap::from([("name_en".to_string(), "Jacob [Collector]".to_string())]),
        ),
    ]);
    extract_vendor_catalogs_from_packet_log(&input, temp.path(), &localized_npc_names)?;

    let collectors: serde_json::Value = serde_json::from_slice(&std::fs::read(
        temp.path().join("collectors").join("collectors.json"),
    )?)?;
    assert_eq!(collectors.as_array().map(Vec::len), Some(2));
    assert_eq!(collectors[0]["npc_model_id"], 525);
    assert_eq!(collectors[0]["map_id"], 146);
    assert_eq!(collectors[0]["position"]["x"], 10.0);
    assert_eq!(collectors[0]["name_en"], "Brownlow [Collector]");
    assert_eq!(collectors[0]["offers"][0]["required_item"]["model_id"], 948);
    assert_eq!(collectors[1]["npc_model_id"], 525);
    assert_eq!(collectors[1]["map_id"], 146);
    assert_eq!(collectors[1]["position"]["x"], 30.0);
    assert_eq!(collectors[1]["name_en"], "Jacob [Collector]");
    assert_eq!(collectors[1]["offers"][0]["required_item"]["model_id"], 949);
    let merchants: serde_json::Value = serde_json::from_slice(&std::fs::read(
        temp.path().join("merchants").join("merchants.json"),
    )?)?;
    assert_eq!(merchants[0]["items"][0]["model_id"], 13);
    assert_eq!(merchants[0]["name_en"], "Brownlow [Collector]");
    let crafters: serde_json::Value = serde_json::from_slice(&std::fs::read(
        temp.path().join("crafters").join("crafters.json"),
    )?)?;
    assert_eq!(crafters[0]["items"][0]["model_file_id"], 204);
    assert_eq!(crafters[0]["name_en"], "Brownlow [Collector]");
    let trainers: serde_json::Value = serde_json::from_slice(&std::fs::read(
        temp.path()
            .join("skill_trainers")
            .join("skill_trainers.json"),
    )?)?;
    assert_eq!(trainers[0]["skills"][0]["skill_id"], 900);
    assert_eq!(trainers[0]["name_en"], "Brownlow [Collector]");
    let coverage: serde_json::Value =
        serde_json::from_slice(&std::fs::read(temp.path().join("coverage.json"))?)?;
    assert_eq!(coverage[0]["map_id"], 146);
    assert_eq!(coverage[0]["collectors"].as_array().map(Vec::len), Some(2));
    assert_eq!(coverage[0]["collectors"][0]["npc_model_id"], 525);
    assert_eq!(coverage[0]["merchants"][0]["npc_model_id"], 525);
    Ok(())
}
