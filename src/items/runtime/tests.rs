use super::*;
use crate::tests::TestDir;
use std::fs;

#[test]
fn packet_log_items_json_is_flat_scalar_rows_with_names() -> anyhow::Result<()> {
    let temp = TestDir::new()?;

    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"kind\":\"status\",\"status\":\"hook_installed_stoc_handler_table\"}\n",
            "{\"item_id\":612,\"model_file_id\":111926,\"model_file_id_raw\":2147595574,\"item_type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":536875008,\"price\":5,\"model_id\":32,\"quantity\":1,\"name_id\":8360,\"mod_struct_size\":1,\"mods_hex\":\"00\"}\n",
        ),
    )?;

    assert_eq!(packet_log_name_ids(&packet_log)?, BTreeSet::from([8360]));

    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            8360,
            BTreeMap::from([
                ("en".to_string(), "Black Dye".to_string()),
                ("fr".to_string(), "Teinture noire".to_string()),
            ]),
        )]),
        by_model_file_id: BTreeMap::new(),
    };
    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;
    let items = json
        .as_array()
        .expect("items.json should be a top-level array");
    let item = &items[0];

    assert_eq!(items.len(), 1);
    assert_eq!(item["model_id"].as_u64(), Some(32));
    assert_eq!(item["model_file_id"].as_u64(), Some(111926));
    assert_eq!(item["item_ids"], serde_json::json!([612]));
    assert_eq!(item["packet_name_id"].as_u64(), Some(8360));
    assert_eq!(item["item_type"].as_u64(), Some(3));
    assert_eq!(item["extra_id"].as_u64(), Some(0));
    assert_eq!(item["materials"].as_u64(), Some(0));
    assert_eq!(item["interaction"].as_u64(), Some(536875008));
    assert_eq!(item["price"].as_u64(), Some(5));
    assert_eq!(item["name_en"].as_str(), Some("Black Dye"));
    assert_eq!(item["name_fr"].as_str(), Some("Teinture noire"));
    assert!(item.get("name").is_none());
    assert!(
        item.as_object()
            .unwrap()
            .values()
            .all(|value| !value.is_object())
    );
    assert!(item.get("icon").is_none());
    assert!(item.get("prices").is_none());
    assert!(item.get("item_types").is_none());
    assert!(item.get("mods_hex").is_none());
    assert!(item.get("mod_struct_sizes").is_none());
    assert!(item.get("quantity").is_none());
    assert!(item.get("observations").is_none());

    Ok(())
}

#[test]
fn packet_log_text_decode_ids_rows_feed_name_id_lookup() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        "{\"kind\":\"text_decode_ids\",\"language_id\":0,\"encoded_hex\":\"6401c8010000\",\"decoded_ids\":[100,200]}\n",
    )?;

    assert_eq!(
        packet_log_name_ids(&packet_log)?,
        BTreeSet::from([100, 200])
    );
    Ok(())
}

#[test]
fn packet_log_encoded_names_feed_compact_record_seeds() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        "{\"kind\":\"text_decode_ids\",\"language_id\":8,\"encoded_hex\":\"a82157d18fb56f160000\",\"decoded_ids\":[8360]}\n",
    )?;

    assert_eq!(packet_log_name_ids(&packet_log)?, BTreeSet::from([8360]));
    assert_eq!(
        packet_log_name_seeds(&packet_log)?.get(&8360).copied(),
        Some(21_740_376_426_095)
    );
    Ok(())
}

#[test]
fn encstring_values_cover_asyncdecode_item_samples() {
    let samples = [
        (
            "3d0a0a011d2708afc6bfed670100",
            &[9757_u64][..],
            Some(12_456_565_711_085_u64),
        ),
        (
            "3e0a0a01860a0a01490a010001010e0101000200020102003e0a0a01920a01000200020102003e0a0a018a0a0a01590a01000b01c20a01011c0101000100",
            &[2377, 2, 2450, 2, 2393, 2498],
            None,
        ),
        (
            "3d0a0a01350a010130010a01d822a4a40ded044301000100",
            &[8664],
            Some(9_645_242_365_188_u64),
        ),
        (
            "3b0a0a019a0a01000200020102003e0a0a018a0a0a01590a01000b01c20a0101900101000100",
            &[2458, 2, 2393, 2498],
            None,
        ),
    ];

    for (hex, async_ids, seed) in samples {
        let values = encoded_values_for_test(hex);
        for id in async_ids {
            assert!(values.contains(id), "{hex} should contain decoded id {id}");
        }
        if let Some(seed) = seed {
            assert!(
                values.contains(&seed),
                "{hex} should contain item-name seed"
            );
        }
    }
}

#[test]
fn encstring_value_two_separator_rule_matches_asyncdecode_samples() {
    let samples = [
        (
            "3e0a0a01860a0a01490a010001010e0101000200020102003e0a0a01920a01000200020102003e0a0a018a0a0a01590a01000b01c20a01011c0101000100",
            2,
        ),
        (
            "3b0a0a019a0a01000200020102003e0a0a018a0a0a01590a01000b01c20a0101900101000100",
            1,
        ),
    ];

    for (hex, expected_twos) in samples {
        let (words, spans) = encoded_value_spans_for_test(hex);
        let bracketed_twos = spans
            .iter()
            .filter(|(value, start, end)| {
                *value == 2
                    && *start > 0
                    && *end < words.len()
                    && words[*start - 1] == 0x0002
                    && words[*end] == 0x0002
            })
            .count();
        assert_eq!(bracketed_twos, expected_twos, "{hex}");
    }
}

#[test]
fn encstring_item_subset_predictor_matches_asyncdecode_samples() {
    let samples = [
        ("3d0a0a011d2708afc6bfed670100", &[9757_u32][..]),
        (
            "3e0a0a01860a0a01490a010001010e0101000200020102003e0a0a01920a01000200020102003e0a0a018a0a0a01590a01000b01c20a01011c0101000100",
            &[2377, 2, 2450, 2, 2393, 2498],
        ),
        ("3d0a0a01350a010130010a01d822a4a40ded044301000100", &[8664]),
        (
            "3b0a0a019a0a01000200020102003e0a0a018a0a0a01590a01000b01c20a0101900101000100",
            &[2458, 2, 2393, 2498],
        ),
    ];

    for (hex, expected_ids) in samples {
        assert_eq!(asyncdecode_item_ids_for_test(hex), expected_ids, "{hex}");
    }
}

#[test]
fn encstring_item_subset_predictor_matches_local_asyncdecode_corpus() -> anyhow::Result<()> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/item_async_decode_corpus.jsonl");

    let mut checked = 0_usize;
    for (line_index, line) in fs::read_to_string(&path)?.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let row: serde_json::Value = serde_json::from_str(line)?;
        let hex = row["encoded_hex"].as_str().expect("encoded_hex");
        let expected = row["decoded_ids"]
            .as_array()
            .expect("decoded_ids")
            .iter()
            .map(|value| value.as_u64().expect("decoded id") as u32)
            .collect::<Vec<_>>();
        assert_eq!(
            asyncdecode_item_ids_for_test(hex),
            expected,
            "corpus line {}",
            line_index + 1
        );
        checked += 1;
    }

    assert!(checked >= 1_000, "expected the verbose item corpus");
    Ok(())
}

#[test]
fn packet_log_text_decode_trace_rows_feed_compact_text_record_lookup() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"kind\":\"text_decode_trace\",\"record_hex\":\"0d004200070009cef09b8168e4\",\"output_preview\":\"\\u0001\\u0002\"}\n",
            "{\"kind\":\"text_decode_trace\",\"record_hex\":\"0d004200070009cef09b8168e4\",\"output_preview\":\"Backpack\\u0000\"}\n",
        ),
    )?;
    let decoded_records = packet_log_decoded_text_records(&packet_log)?;

    let map = text_records::parse_text_record_map_with_decoded_records(
        &[
            0x0d, 0x00, 0x42, 0x00, 0x07, 0x00, 0x09, 0xce, 0xf0, 0x9b, 0x81, 0x68, 0xe4,
        ],
        &decoded_records,
    )?;

    assert_eq!(map.get(&0).map(String::as_str), Some("Backpack\0"));
    Ok(())
}

#[test]
fn compact_record_seed_decodes_without_client_string_output() -> anyhow::Result<()> {
    let map = text_records::parse_text_record_map_with_decoded_records_and_seeds(
        &[
            0x0d, 0x00, 0x42, 0x00, 0x07, 0x00, 0x09, 0xce, 0xf0, 0x9b, 0x81, 0x68, 0xe4,
        ],
        &BTreeMap::new(),
        &BTreeMap::from([(0, 21_740_376_426_095)]),
    )?;

    assert_eq!(map.get(&0).map(String::as_str), Some("Backpack"));
    Ok(())
}

#[test]
fn compact_record_seed_decodes_japanese_width_16() -> anyhow::Result<()> {
    let map = text_records::parse_text_record_map_with_decoded_records_and_seeds(
        &[
            0x1c, 0x00, 0x6e, 0x30, 0x10, 0x00, 0x1a, 0x56, 0x3f, 0x66, 0xbb, 0xe4, 0xae, 0xf0,
            0x01, 0xae, 0x7e, 0x38, 0xb3, 0xae, 0xe0, 0x77, 0xfd, 0x34, 0xa5, 0x24, 0x74, 0x3f,
        ],
        &BTreeMap::new(),
        &BTreeMap::from([(0, 31_322_958_140_860)]),
    )?;

    assert_eq!(
        map.get(&0).map(String::as_str),
        Some("スクロール：勇者の洞察")
    );
    Ok(())
}

#[test]
fn packet_log_items_json_deduplicates_model_file_variant_and_prefers_named_row()
-> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"item_id\":1,\"model_file_id\":222,\"item_type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":0,\"price\":0,\"model_id\":32}\n",
            "{\"item_id\":2,\"model_file_id\":222,\"item_type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":0,\"price\":0,\"model_id\":32,\"name_id\":8360}\n",
        ),
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            8360,
            BTreeMap::from([("en".to_string(), "Black Dye".to_string())]),
        )]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;
    let items = json.as_array().unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["model_id"].as_u64(), Some(32));
    assert_eq!(items[0]["model_file_id"].as_u64(), Some(222));
    assert_eq!(items[0]["name_en"].as_str(), Some("Black Dye"));
    assert_eq!(items[0]["item_ids"], serde_json::json!([1, 2]));
    assert_eq!(items[0]["observed_variants"].as_array().unwrap().len(), 2);
    Ok(())
}

#[test]
fn packet_log_items_json_keeps_distinct_model_file_variants() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"item_id\":1,\"model_file_id\":111,\"item_type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":0,\"price\":0,\"model_id\":32,\"name_id\":100}\n",
            "{\"item_id\":2,\"model_file_id\":222,\"item_type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":0,\"price\":0,\"model_id\":32,\"name_id\":200}\n",
        ),
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([
            (
                100,
                BTreeMap::from([("en".to_string(), "Variant A".to_string())]),
            ),
            (
                200,
                BTreeMap::from([("en".to_string(), "Variant B".to_string())]),
            ),
        ]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;
    let items = json.as_array().unwrap();

    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["model_file_id"].as_u64(), Some(111));
    assert_eq!(items[0]["name_en"].as_str(), Some("Variant A"));
    assert_eq!(items[1]["model_file_id"].as_u64(), Some(222));
    assert_eq!(items[1]["name_en"].as_str(), Some("Variant B"));
    Ok(())
}

#[test]
fn packet_log_names_can_fall_back_to_model_file_table() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        "{\"item_id\":1,\"model_file_id\":111926,\"item_type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":536875008,\"price\":5,\"model_id\":32}\n",
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::new(),
        by_model_file_id: BTreeMap::from([(
            111926,
            BTreeMap::from([("en".to_string(), "Black Dye".to_string())]),
        )]),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;

    assert_eq!(json[0]["name_en"].as_str(), Some("Black Dye"));
    Ok(())
}

#[test]
fn packet_log_exact_name_ids_fill_missing_languages_from_model_file_table() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        "{\"item_id\":1,\"model_file_id\":111926,\"item_type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":536875008,\"price\":5,\"model_id\":32,\"name_id\":100,\"enc_name_hex\":\"64010000\"}\n",
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            100,
            BTreeMap::from([("en".to_string(), "Black Dye".to_string())]),
        )]),
        by_model_file_id: BTreeMap::from([(
            111926,
            BTreeMap::from([
                ("en".to_string(), "Black Dye".to_string()),
                ("ja".to_string(), "黒染料".to_string()),
            ]),
        )]),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;

    assert_eq!(json[0]["name_en"].as_str(), Some("Black Dye"));
    assert_eq!(json[0]["name_ja"].as_str(), Some("黒染料"));
    Ok(())
}

#[test]
fn compact_item_row_resolves_name_and_description_text_ids() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_items.jsonl");
    fs::write(
        &packet_log,
        "{\"model_id\":32,\"model_file_id\":111926,\"item_type\":3,\"materials\":0,\"name_text_id\":100,\"enc_name_hex\":\"64010000\",\"desc_enc_hex\":\"64010000\"}\n",
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            100,
            BTreeMap::from([("en".to_string(), "Black Dye".to_string())]),
        )]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;
    let item = json[0].as_object().unwrap();

    assert_eq!(item["model_id"], 32);
    assert_eq!(item["model_file_id"], 111926);
    assert_eq!(item["packet_name_id"], 100);
    assert_eq!(item["name_en"], "Black Dye");
    assert_eq!(item["description_en"], "Black Dye");
    assert!(!item.contains_key("item_ids"));
    assert!(!item.contains_key("price"));
    Ok(())
}

#[test]
fn packet_log_encoded_name_hex_expands_template_names() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        "{\"item_id\":2,\"model_file_id\":111926,\"item_type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":536875008,\"price\":5,\"model_id\":32,\"name_text_id\":100,\"enc_name_hex\":\"6401c8010000\"}\n",
    )?;
    assert_eq!(
        packet_log_name_ids(&packet_log)?,
        BTreeSet::from([100, 200])
    );

    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([
            (
                100,
                BTreeMap::from([
                    ("en".to_string(), "%str1 Dye".to_string()),
                    ("fr".to_string(), "Teinture %str1".to_string()),
                ]),
            ),
            (
                200,
                BTreeMap::from([
                    ("en".to_string(), "Black".to_string()),
                    ("fr".to_string(), "noire".to_string()),
                ]),
            ),
        ]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;

    assert_eq!(json[0]["name_en"].as_str(), Some("Black Dye"));
    assert_eq!(json[0]["name_fr"].as_str(), Some("Teinture noire"));
    assert!(json[0].get("enc_name_hex").is_none());
    Ok(())
}

#[test]
fn python_itemgeneral_rows_match_runtime_export_schema() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("item_general_observations.jsonl");
    fs::write(
        &packet_log,
        "{\"item_id\":40,\"model_file_id\":2147595574,\"type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":536875008,\"price\":5,\"model_id\":32,\"quantity\":1,\"decoded_name\":\"Teinture noire\",\"enc_name_hex\":\"a82157d18fb56f160000\"}\n",
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            8360,
            BTreeMap::from([
                ("en".to_string(), "Black Dye".to_string()),
                ("fr".to_string(), "Teinture noire".to_string()),
            ]),
        )]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;

    assert_eq!(json[0]["model_file_id"].as_u64(), Some(111926));
    assert_eq!(json[0]["item_type"].as_u64(), Some(3));
    assert_eq!(json[0]["name_en"].as_str(), Some("Black Dye"));
    assert_eq!(json[0]["name_fr"].as_str(), Some("Teinture noire"));
    Ok(())
}

#[test]
fn packet_log_desc_enc_hex_expands_description_templates() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        "{\"item_id\":2,\"model_file_id\":111926,\"item_type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":536875008,\"price\":5,\"model_id\":32,\"name_id\":100,\"enc_name_hex\":\"64010000\",\"desc_enc_hex\":\"2c0290020000\"}\n",
    )?;
    assert!(packet_log_name_ids(&packet_log)?.is_superset(&BTreeSet::from([100, 300, 400])));
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([
            (
                100,
                BTreeMap::from([("en".to_string(), "Black Dye".to_string())]),
            ),
            (
                300,
                BTreeMap::from([
                    ("en".to_string(), "Use: %str1.".to_string()),
                    ("fr".to_string(), "Utilisation : %str1.".to_string()),
                ]),
            ),
            (
                400,
                BTreeMap::from([
                    ("en".to_string(), "opens a kit".to_string()),
                    ("fr".to_string(), "ouvre un nécessaire".to_string()),
                ]),
            ),
        ]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;

    assert_eq!(
        json[0]["description_en"].as_str(),
        Some("Use: opens a kit.")
    );
    assert_eq!(
        json[0]["description_fr"].as_str(),
        Some("Utilisation : ouvre un nécessaire.")
    );
    Ok(())
}

#[test]
fn runtime_item_strings_attach_description_without_extra_item_row() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"item_id\":2,\"model_file_id\":111926,\"item_type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":536875008,\"price\":5,\"model_id\":32,\"name_id\":100,\"enc_name_hex\":\"64010000\",\"decoded_ids\":[100]}\n",
            "{\"kind\":\"runtime_item_strings\",\"item_id\":2,\"model_id\":32,\"model_file_id\":111926,\"desc_complete\":true,\"desc_enc_hex\":\"2c0290020000\"}\n",
        ),
    )?;
    assert!(packet_log_name_ids(&packet_log)?.is_superset(&BTreeSet::from([100, 300, 400])));
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([
            (
                100,
                BTreeMap::from([("en".to_string(), "Black Dye".to_string())]),
            ),
            (
                300,
                BTreeMap::from([("en".to_string(), "Use: %str1.".to_string())]),
            ),
            (
                400,
                BTreeMap::from([("en".to_string(), "opens a kit".to_string())]),
            ),
        ]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;
    let items = json.as_array().unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["item_type"].as_u64(), Some(3));
    assert_eq!(items[0]["name_en"].as_str(), Some("Black Dye"));
    assert_eq!(
        items[0]["runtime_description_available"].as_bool(),
        Some(true)
    );
    assert_eq!(
        items[0]["description_en"].as_str(),
        Some("Use: opens a kit.")
    );
    Ok(())
}

#[test]
fn runtime_item_strings_complete_name_can_name_item() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"item_id\":2,\"model_file_id\":111926,\"item_type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":536875008,\"price\":5,\"model_id\":32}\n",
            "{\"kind\":\"runtime_item_strings\",\"item_id\":2,\"model_id\":32,\"model_file_id\":111926,\"desc_complete\":false,\"complete_name_enc_hex\":\"2c0264010000\"}\n",
        ),
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([
            (
                100,
                BTreeMap::from([("en".to_string(), "Black Dye".to_string())]),
            ),
            (
                300,
                BTreeMap::from([("en".to_string(), "%str1%".to_string())]),
            ),
        ]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;
    let item = json.as_array().unwrap().first().unwrap();

    assert_eq!(item["name_en"].as_str(), Some("Black Dye"));
    assert_eq!(item["runtime_description_available"].as_bool(), Some(false));
    Ok(())
}

#[test]
fn text_decode_ids_match_runtime_hex_with_extra_null() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"item_id\":2,\"model_file_id\":111926,\"item_type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":536875008,\"price\":5,\"model_id\":32}\n",
            "{\"kind\":\"runtime_item_strings\",\"item_id\":2,\"model_id\":32,\"model_file_id\":111926,\"complete_name_enc_hex\":\"aaaa\"}\n",
            "{\"kind\":\"text_decode_ids\",\"language_id\":0,\"encoded_hex\":\"aaaa0000\",\"decoded_ids\":[100]}\n",
        ),
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            100,
            BTreeMap::from([("en".to_string(), "Black Dye".to_string())]),
        )]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;
    let item = json.as_array().unwrap().first().unwrap();

    assert_eq!(item["name_en"].as_str(), Some("Black Dye"));
    Ok(())
}

#[test]
fn decoded_description_rows_attach_multilingual_descriptions() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"item_id\":2,\"model_file_id\":111926,\"item_type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":536875008,\"price\":5,\"model_id\":32,\"name_id\":100,\"enc_name_hex\":\"64010000\",\"decoded_ids\":[100]}\n",
            "{\"kind\":\"decoded_description\",\"item_id\":2,\"model_id\":32,\"model_file_id\":111926,\"lang\":\"en\",\"description\":\"Use: opens a kit.\"}\n",
            "{\"kind\":\"decoded_description\",\"item_id\":2,\"model_id\":32,\"model_file_id\":111926,\"lang\":\"fr\",\"description\":\"Utilisation : ouvre un nécessaire.\"}\n",
        ),
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            100,
            BTreeMap::from([("en".to_string(), "Black Dye".to_string())]),
        )]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;
    let item = json.as_array().unwrap().first().unwrap();

    assert_eq!(item["description_en"].as_str(), Some("Use: opens a kit."));
    assert_eq!(
        item["description_fr"].as_str(),
        Some("Utilisation : ouvre un nécessaire.")
    );
    Ok(())
}

#[test]
fn packet_log_merged_single_decoded_id_can_name_item() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        "{\"item_id\":6508,\"model_file_id\":152714,\"item_type\":22,\"extra_id\":0,\"materials\":0,\"interaction\":0,\"price\":0,\"model_id\":6508,\"name_id\":9396,\"enc_name_hex\":\"b425cfe100ef786f0000\",\"decoded_ids\":[9396]}\n",
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            9396,
            BTreeMap::from([("en".to_string(), "Sceptre luminescent".to_string())]),
        )]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;

    assert_eq!(json[0]["name_en"].as_str(), Some("Sceptre luminescent"));
    Ok(())
}

#[test]
fn unresolved_encoded_name_hex_does_not_use_unsafe_local_fallback() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        "{\"item_id\":6508,\"model_file_id\":152714,\"item_type\":22,\"extra_id\":0,\"materials\":0,\"interaction\":0,\"price\":0,\"model_id\":6508,\"name_id\":9396,\"enc_name_hex\":\"b425cfe100ef786f0000\"}\n",
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            9396,
            BTreeMap::from([("en".to_string(), "Wrong local fallback".to_string())]),
        )]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;

    let item = json[0].as_object().unwrap();
    assert!(item.get("name").is_none());
    assert!(item.keys().all(|key| !key.starts_with("name_")));
    Ok(())
}

#[test]
fn unresolved_encoded_name_hex_can_use_model_file_fallback() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        "{\"item_id\":6508,\"model_file_id\":152714,\"item_type\":22,\"extra_id\":0,\"materials\":0,\"interaction\":0,\"price\":0,\"model_id\":6508,\"name_id\":9396,\"enc_name_hex\":\"b425cfe100ef786f0000\"}\n",
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            9396,
            BTreeMap::from([("en".to_string(), "Wrong local fallback".to_string())]),
        )]),
        by_model_file_id: BTreeMap::from([(
            152714,
            BTreeMap::from([("en".to_string(), "Safe model-file fallback".to_string())]),
        )]),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;

    assert_eq!(
        json[0]["name_en"].as_str(),
        Some("Safe model-file fallback")
    );
    Ok(())
}

#[test]
fn resolved_text_id_can_name_compact_item() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        "{\"item_id\":6508,\"model_file_id\":152714,\"item_type\":22,\"extra_id\":0,\"materials\":0,\"interaction\":0,\"price\":0,\"model_id\":6508,\"name_id\":9396,\"enc_name_hex\":\"b425cfe100ef786f0000\"}\n",
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::from([9396]),
        by_text_id: BTreeMap::from([(
            9396,
            BTreeMap::from([("en".to_string(), "Sceptre luminescent".to_string())]),
        )]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;

    assert_eq!(json[0]["name_en"].as_str(), Some("Sceptre luminescent"));
    Ok(())
}

#[test]
fn packet_log_uses_official_multilingual_names_for_known_itemgeneral_samples() -> anyhow::Result<()>
{
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"item_id\":44,\"model_file_id\":79229,\"model_file_id_raw\":2147562877,\"item_type\":29,\"extra_id\":0,\"materials\":0,\"interaction\":553648641,\"price\":28,\"model_id\":2992,\"quantity\":1,\"name_id\":9757,\"enc_name_hex\":\"1d2708afc6bfed670000\"}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":44,\"lang\":\"fr\",\"name\":\"Nécessaire de recyclage\"}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":44,\"lang\":\"en\",\"name\":\"Salvage Kit\"}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":44,\"lang\":\"de\",\"name\":\"Wiederverwertungswerkzeug\"}\n",
            "{\"item_id\":56,\"model_file_id\":86521,\"model_file_id_raw\":2147570169,\"item_type\":11,\"extra_id\":0,\"materials\":0,\"interaction\":537395745,\"price\":3,\"model_id\":929,\"quantity\":48,\"name_id\":8664,\"enc_name_hex\":\"d822a4a40ded04430000\"}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":56,\"lang\":\"fr\",\"name\":\"Tas de Poussière scintillante\"}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":56,\"lang\":\"en\",\"name\":\"Pile of Glittering Dust\"}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":56,\"lang\":\"de\",\"name\":\"Glitzerstaubhaufen\"}\n",
        ),
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([
            (
                9757,
                BTreeMap::from([("en".to_string(), "Wrong salvage fallback".to_string())]),
            ),
            (
                8664,
                BTreeMap::from([("en".to_string(), "Wrong dust fallback".to_string())]),
            ),
        ]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;
    let by_model_id = json
        .as_array()
        .unwrap()
        .iter()
        .map(|item| (item["model_id"].as_u64().unwrap(), item))
        .collect::<BTreeMap<_, _>>();

    let salvage = by_model_id[&2992];
    assert_eq!(salvage["model_file_id"].as_u64(), Some(79229));
    assert_eq!(salvage["name_fr"].as_str(), Some("Nécessaire de recyclage"));
    assert_eq!(salvage["name_en"].as_str(), Some("Salvage Kit"));
    assert_eq!(
        salvage["name_de"].as_str(),
        Some("Wiederverwertungswerkzeug")
    );

    let dust = by_model_id[&929];
    assert_eq!(dust["model_file_id"].as_u64(), Some(86521));
    assert_eq!(
        dust["name_fr"].as_str(),
        Some("Tas de Poussière scintillante")
    );
    assert_eq!(dust["name_en"].as_str(), Some("Pile of Glittering Dust"));
    assert_eq!(dust["name_de"].as_str(), Some("Glitzerstaubhaufen"));
    for item in [salvage, dust] {
        assert!(item.get("name").is_none());
        assert!(
            item.as_object()
                .unwrap()
                .values()
                .all(|value| !value.is_object())
        );
    }
    Ok(())
}

#[test]
fn official_decoded_names_override_unsafe_local_guesses() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"item_id\":109,\"model_file_id\":176191,\"item_type\":30,\"extra_id\":0,\"materials\":443,\"interaction\":537395777,\"price\":25,\"model_id\":822,\"name_id\":21992,\"enc_name_hex\":\"e85649eacf9d7b240000\"}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":109,\"lang\":\"en\",\"name\":\"Corne de gardien\"}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":109,\"lang\":\"fr\",\"name\":\"Corne de gardien\"}\n",
        ),
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            21992,
            BTreeMap::from([("en".to_string(), "Kaineng Docks".to_string())]),
        )]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;

    assert_eq!(json[0]["name"].as_str(), Some("Corne de gardien"));
    assert!(json[0].get("name_en").is_none());
    Ok(())
}

#[test]
fn client_decoded_names_are_opt_in() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"item_id\":1,\"model_file_id\":11,\"item_type\":3,\"extra_id\":0,\"materials\":0,\"interaction\":0,\"price\":0,\"model_id\":22,\"name_id\":100}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":1,\"lang\":\"en\",\"name\":\"Client Name\"}\n",
        ),
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            100,
            BTreeMap::from([("en".to_string(), "Dat Name".to_string())]),
        )]),
        by_model_file_id: BTreeMap::new(),
    };

    let dat_only = temp.path().join("dat-only.json");
    export_detected_items_from_packet_log_with_client_strings(
        &packet_log,
        &names,
        &dat_only,
        false,
    )?;
    let json: serde_json::Value = serde_json::from_reader(File::open(dat_only)?)?;
    assert_eq!(json[0]["name_en"].as_str(), Some("Dat Name"));
    assert!(json[0].get("name").is_none());

    let client_strings = temp.path().join("client-strings.json");
    export_detected_items_from_packet_log_with_client_strings(
        &packet_log,
        &names,
        &client_strings,
        true,
    )?;
    let json: serde_json::Value = serde_json::from_reader(File::open(client_strings)?)?;
    assert_eq!(json[0]["name"].as_str(), Some("Client Name"));
    Ok(())
}

#[test]
fn official_decoded_names_match_reused_item_id_by_model() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"item_id\":77,\"model_file_id\":1001,\"item_type\":2,\"extra_id\":0,\"materials\":0,\"interaction\":0,\"price\":0,\"model_id\":101}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":77,\"model_id\":101,\"model_file_id\":1001,\"lang\":\"en\",\"name\":\"First Model\"}\n",
            "{\"item_id\":77,\"model_file_id\":2002,\"item_type\":2,\"extra_id\":0,\"materials\":0,\"interaction\":0,\"price\":0,\"model_id\":202}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":77,\"model_id\":202,\"model_file_id\":2002,\"lang\":\"en\",\"name\":\"Second Model\"}\n",
        ),
    )?;
    let names = RuntimeTextLookup::default();

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;
    let by_model_id = json
        .as_array()
        .unwrap()
        .iter()
        .map(|item| (item["model_id"].as_u64().unwrap(), item["name"].as_str()))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(by_model_id[&101], Some("First Model"));
    assert_eq!(by_model_id[&202], Some("Second Model"));
    Ok(())
}

#[test]
fn official_current_language_can_validate_local_multilingual_names() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"item_id\":200,\"model_file_id\":97340,\"item_type\":2,\"extra_id\":0,\"materials\":452,\"interaction\":780141888,\"price\":51,\"model_id\":114,\"name_id\":1000}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":200,\"lang\":\"en\",\"name\":\"Hache de Nain\"}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":200,\"lang\":\"fr\",\"name\":\"Hache de Nain\"}\n",
        ),
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            1000,
            BTreeMap::from([
                ("en".to_string(), "Dwarven Axe".to_string()),
                ("fr".to_string(), "Hache de Nain".to_string()),
            ]),
        )]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;

    assert_eq!(json[0]["name_en"].as_str(), Some("Dwarven Axe"));
    assert_eq!(json[0]["name_fr"].as_str(), Some("Hache de Nain"));
    assert!(json[0].get("name").is_none());
    Ok(())
}

#[test]
fn official_current_language_can_validate_name_id_after_encoded_decode_miss() -> anyhow::Result<()>
{
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"item_id\":367,\"model_file_id\":152714,\"item_type\":22,\"extra_id\":0,\"materials\":0,\"interaction\":838865152,\"price\":0,\"model_id\":6508,\"name_id\":9396,\"enc_name_hex\":\"bad\"}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":367,\"lang\":\"en\",\"name\":\"Sceptre luminescent\"}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":367,\"lang\":\"fr\",\"name\":\"Sceptre luminescent\"}\n",
        ),
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            9396,
            BTreeMap::from([
                ("en".to_string(), "Luminescent Scepter".to_string()),
                ("fr".to_string(), "Sceptre luminescent".to_string()),
            ]),
        )]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;

    assert_eq!(json[0]["name_en"].as_str(), Some("Luminescent Scepter"));
    assert_eq!(json[0]["name_fr"].as_str(), Some("Sceptre luminescent"));
    assert!(json[0].get("name").is_none());
    Ok(())
}

#[test]
fn official_generic_names_override_invalid_client_rows() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let packet_log = temp.path().join("tyria_packets.jsonl");
    fs::write(
        &packet_log,
        concat!(
            "{\"item_id\":19,\"model_file_id\":176773,\"item_type\":26,\"extra_id\":0,\"materials\":0,\"interaction\":572522752,\"price\":0,\"model_id\":24380,\"name_id\":8326,\"enc_name_hex\":\"86210000\"}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":19,\"lang\":\"en\",\"name\":\"Inconnu\"}\n",
            "{\"kind\":\"decoded_name\",\"item_id\":19,\"lang\":\"fr\",\"name\":\"Inconnu\"}\n",
        ),
    )?;
    let names = RuntimeTextLookup {
        exact_text_ids: BTreeSet::new(),
        by_text_id: BTreeMap::from([(
            8326,
            BTreeMap::from([
                ("en".to_string(), "Unknown".to_string()),
                ("fr".to_string(), "Inconnu".to_string()),
            ]),
        )]),
        by_model_file_id: BTreeMap::new(),
    };

    let out = temp.path().join("items/items.json");
    export_detected_items_from_packet_log(&packet_log, &names, &out)?;
    let json: serde_json::Value = serde_json::from_reader(File::open(out)?)?;

    assert!(json[0].get("name").is_none());
    assert_eq!(json[0]["name_en"].as_str(), Some("Unknown"));
    assert_eq!(json[0]["name_fr"].as_str(), Some("Inconnu"));
    Ok(())
}
