use super::*;

#[test]
fn utf16le_strings_extracts_offsets_and_ignores_short_noise() {
    let mut bytes = Vec::new();
    for code in [b'A' as u16, b'b' as u16, b'c' as u16] {
        bytes.extend_from_slice(&code.to_le_bytes());
    }
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    let wanted_offset = bytes.len();
    for code in "Skill text".encode_utf16() {
        bytes.extend_from_slice(&code.to_le_bytes());
    }
    bytes.extend_from_slice(&0_u16.to_le_bytes());

    let strings = utf16le_strings(&bytes);

    assert_eq!(strings.len(), 1);
    assert_eq!(strings[0].0, wanted_offset);
    assert_eq!(strings[0].1, "Skill text");
}

#[test]
fn utf16le_strings_finds_odd_aligned_string_at_real_byte_offset() {
    let mut bytes = vec![0xff];
    let wanted_offset = bytes.len();
    for code in "Odd skill text".encode_utf16() {
        bytes.extend_from_slice(&code.to_le_bytes());
    }
    bytes.extend_from_slice(&0_u16.to_le_bytes());

    let strings = utf16le_strings(&bytes);

    assert_eq!(strings, vec![(wanted_offset, "Odd skill text".to_string())]);
}

#[test]
fn utf16le_strings_drops_gw_resource_printable_trailer() {
    let mut bytes = Vec::new();
    let wanted_offset = bytes.len();
    for code in "Flame Burst".encode_utf16() {
        bytes.extend_from_slice(&code.to_le_bytes());
    }
    bytes.extend_from_slice(&(b'j' as u16).to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&0x10_u16.to_le_bytes());
    for code in "All nearby foes".encode_utf16() {
        bytes.extend_from_slice(&code.to_le_bytes());
    }
    bytes.extend_from_slice(&0_u16.to_le_bytes());

    let strings = utf16le_strings(&bytes);

    assert!(strings.contains(&(wanted_offset, "Flame Burst".to_string())));
    assert!(!strings.iter().any(|(_, text)| text == "Flame Burstj"));
}

fn utf16le_payload(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    for code in text.encode_utf16() {
        bytes.extend_from_slice(&code.to_le_bytes());
    }
    bytes
}

fn text_record_bytes(
    compression_or_flags: u16,
    record_type: u8,
    record_subtype: u8,
    payload: &[u8],
) -> Vec<u8> {
    let size = 6 + payload.len();
    assert!(u16::try_from(size).is_ok());

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(size as u16).to_le_bytes());
    bytes.extend_from_slice(&compression_or_flags.to_le_bytes());
    bytes.push(record_type);
    bytes.push(record_subtype);
    bytes.extend_from_slice(payload);
    bytes
}

#[test]
fn text_record_parser_recovers_shifted_uncompressed_utf16le_entries_and_map_keys()
-> anyhow::Result<()> {
    let mut bytes = vec![0, 0, 0xff];
    bytes.extend_from_slice(&text_record_bytes(0, 0x20, 0, b"meta"));
    let first_record_start = bytes.len();
    let first_payload = utf16le_payload("First skill line");
    bytes.extend_from_slice(&text_record_bytes(0, 0x10, 0, &first_payload));
    let second_record_start = bytes.len();
    let second_payload = utf16le_payload("Second skill line");
    bytes.extend_from_slice(&text_record_bytes(0, 0x10, 0, &second_payload));

    let records = text_records::parse_text_record_entries(&bytes)?;

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].record_start, first_record_start);
    assert_eq!(records[0].record_size, 6 + first_payload.len());
    assert_eq!(records[0].compression_or_flags, 0);
    assert_eq!(records[0].record_type, 0x10);
    assert_eq!(records[0].record_subtype, 0);
    assert_eq!(records[0].ordinal, 0);
    assert_eq!(records[0].record_index, 1);
    assert_eq!(records[0].text, "First skill line");
    assert_eq!(records[1].record_start, second_record_start);
    assert_eq!(records[1].ordinal, 1);
    assert_eq!(records[1].record_index, 2);
    assert_eq!(records[1].text, "Second skill line");

    let map = text_records::parse_text_record_map(&bytes)?;
    assert_eq!(map.get(&0), None);
    assert_eq!(map.get(&1).map(String::as_str), Some("First skill line"));
    assert_eq!(map.get(&2).map(String::as_str), Some("Second skill line"));

    Ok(())
}
