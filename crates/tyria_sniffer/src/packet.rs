use std::io::{self, Write};

const ITEM_GENERAL_FIXED_BYTES: usize = 48;
const ITEM_GENERAL_MOD_SIZE_OFFSET: usize = 176;
const ITEM_GENERAL_MODS_START: usize = 180;
const MAX_ITEM_MOD_WORDS: u32 = 64;
pub(crate) const ITEM_GENERAL_NAME_START: usize = 48;
const MAX_ENCODED_NAME_BYTES: usize = 192;

pub(crate) fn guessed_packet_size(header: u32) -> usize {
    match header {
        0x0056 => 44,
        0x0057 => 44,
        0x009B => 88,
        0x00AA => 0x68,
        0x015F => 40,
        0x0161 | 0x0162 => 0x1b4,
        _ => 256,
    }
}

#[cfg(test)]
pub(crate) fn header_name(header: u32) -> &'static str {
    match header {
        0x0056 => "NPC_UPDATE_PROPERTIES",
        0x0057 => "NPC_UPDATE_MODEL",
        0x006D => "NPC_UPDATE_WEAPONS",
        0x009B => "AGENT_UPDATE_NPC_NAME",
        0x00AA => "AGENT_CREATE_NPC",
        0x015F => "CREATE_UNNAMED_ITEM",
        0x0161 => "ITEM_GENERAL_INFO",
        0x0162 => "ITEM_REUSE_ID",
        _ => "UNKNOWN",
    }
}

pub(crate) fn write_decoded(out: &mut impl Write, header: u32, data: &[u8]) -> io::Result<bool> {
    match header {
        0x0161 | 0x0162 if data.len() >= ITEM_GENERAL_FIXED_BYTES => {
            write_item_general_decoded(out, data)?;
            Ok(true)
        }
        0x015F if data.len() >= 40 => {
            write_unnamed_item_decoded(out, data)?;
            Ok(true)
        }
        0x0056 if data.len() >= 44 => {
            write_npc_properties_decoded(out, data)?;
            Ok(true)
        }
        0x0057 if data.len() >= 44 => {
            write_npc_model_decoded(out, data)?;
            Ok(true)
        }
        0x009B if data.len() >= 88 => {
            write_agent_name_decoded(out, data)?;
            Ok(true)
        }
        0x00AA if data.len() >= 0x68 => {
            write_agent_create_decoded(out, data)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub(crate) fn write_hex(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    const BYTES_PER_CHUNK: usize = 256;

    let mut encoded = [0u8; BYTES_PER_CHUNK * 2];
    for chunk in bytes.chunks(BYTES_PER_CHUNK) {
        for (index, &byte) in chunk.iter().enumerate() {
            encoded[index * 2] = HEX[(byte >> 4) as usize];
            encoded[index * 2 + 1] = HEX[(byte & 0x0F) as usize];
        }
        out.write_all(&encoded[..chunk.len() * 2])?;
    }
    Ok(())
}

pub(crate) fn write_u16_hex(out: &mut impl Write, words: &[u16]) -> io::Result<()> {
    const WORDS_PER_CHUNK: usize = 128;

    let mut bytes = [0u8; WORDS_PER_CHUNK * size_of::<u16>()];
    for chunk in words.chunks(WORDS_PER_CHUNK) {
        for (index, word) in chunk.iter().enumerate() {
            let offset = index * size_of::<u16>();
            bytes[offset..offset + 2].copy_from_slice(&word.to_le_bytes());
        }
        write_hex(out, &bytes[..std::mem::size_of_val(chunk)])?;
    }
    Ok(())
}

fn write_item_common_decoded(out: &mut impl Write, data: &[u8]) -> io::Result<()> {
    let model_file_id_raw = u32_at(data, 8);
    write!(
        out,
        "{{\"item_id\":{},\"model_file_id\":{},\"model_file_id_raw\":{},\"item_type\":{},\"unk1\":{},\"extra_id\":{},\"materials\":{},\"unk2\":{},\"interaction\":{},\"price\":{}",
        u32_at(data, 4),
        model_file_id_raw & 0x7FFF_FFFF,
        model_file_id_raw,
        u32_at(data, 12),
        u32_at(data, 16),
        u32_at(data, 20),
        u32_at(data, 24),
        u32_at(data, 28),
        u32_at(data, 32),
        u32_at(data, 36)
    )
}

fn write_item_general_decoded(out: &mut impl Write, data: &[u8]) -> io::Result<()> {
    write_item_common_decoded(out, data)?;
    write!(
        out,
        ",\"model_id\":{},\"quantity\":{}",
        u32_at(data, 40),
        u32_at(data, 44)
    )?;

    if let Some(name_id) = encoded_u32_at(data, ITEM_GENERAL_NAME_START) {
        write!(out, ",\"name_id\":{},\"enc_name_hex\":\"", name_id)?;
        write_hex(out, encoded_name_bytes(data, ITEM_GENERAL_NAME_START))?;
        write!(out, "\"")?;
    }

    if data.len() >= ITEM_GENERAL_MODS_START {
        let mod_struct_size = u32_at(data, ITEM_GENERAL_MOD_SIZE_OFFSET);
        let mod_bytes = mod_struct_size.min(MAX_ITEM_MOD_WORDS) as usize * size_of::<u32>();
        let mods_end = data.len().min(ITEM_GENERAL_MODS_START + mod_bytes);
        write!(
            out,
            ",\"mod_struct_size\":{},\"mods_hex\":\"",
            mod_struct_size
        )?;
        write_hex(out, &data[ITEM_GENERAL_MODS_START..mods_end])?;
        write!(out, "\"")?;
    }

    write!(out, "}}")
}

fn write_unnamed_item_decoded(out: &mut impl Write, data: &[u8]) -> io::Result<()> {
    write_item_common_decoded(out, data)?;
    write!(out, "}}")
}

fn write_npc_properties_decoded(out: &mut impl Write, data: &[u8]) -> io::Result<()> {
    write!(
        out,
        "{{\"npc_id\":{},\"file_id\":{},\"data1\":{},\"scale\":{},\"data2\":{},\"flags\":{},\"profession\":{},\"level\":{},\"name_enc_hex\":\"",
        u32_at(data, 4),
        u32_at(data, 8),
        u32_at(data, 12),
        u32_at(data, 16),
        u32_at(data, 20),
        u32_at(data, 24),
        u32_at(data, 28),
        u32_at(data, 32)
    )?;
    write_hex(out, &data[36..44])?;
    write!(out, "\"}}")
}

fn write_npc_model_decoded(out: &mut impl Write, data: &[u8]) -> io::Result<()> {
    write!(
        out,
        "{{\"npc_id\":{},\"count\":{},\"model_file_ids\":[{},{},{},{},{},{},{},{}]}}",
        u32_at(data, 4),
        u32_at(data, 8),
        u32_at(data, 12),
        u32_at(data, 16),
        u32_at(data, 20),
        u32_at(data, 24),
        u32_at(data, 28),
        u32_at(data, 32),
        u32_at(data, 36),
        u32_at(data, 40)
    )
}

fn write_agent_name_decoded(out: &mut impl Write, data: &[u8]) -> io::Result<()> {
    write!(
        out,
        "{{\"agent_id\":{},\"name_enc_hex\":\"",
        u32_at(data, 4)
    )?;
    write_hex(out, &data[8..88])?;
    write!(out, "\"}}")
}

fn write_agent_create_decoded(out: &mut impl Write, data: &[u8]) -> io::Result<()> {
    write!(
        out,
        "{{\"agent_id\":{},\"agent_type\":{},\"type\":{},\"x\":{},\"y\":{},\"speed\":{},\"allegiance_bits\":{}}}",
        u32_at(data, 4),
        u32_at(data, 8),
        u32_at(data, 12),
        f32_at(data, 20),
        f32_at(data, 24),
        f32_at(data, 40),
        u32_at(data, 52)
    )
}

pub(crate) fn encoded_name_bytes(data: &[u8], offset: usize) -> &[u8] {
    if offset >= data.len() {
        return &[];
    }
    let mut end = std::cmp::min(data.len(), offset.saturating_add(MAX_ENCODED_NAME_BYTES));
    let mut cursor = offset;
    while cursor + 2 <= end {
        if u16::from_le_bytes([data[cursor], data[cursor + 1]]) == 0 {
            end = cursor + 2;
            break;
        }
        cursor += 2;
    }
    &data[offset..end]
}

pub(crate) fn encoded_u32_at(data: &[u8], offset: usize) -> Option<u32> {
    let mut value = 0_u64;
    let mut cursor = offset;
    loop {
        let bytes = data.get(cursor..)?.get(..2)?;
        let word = u16::from_le_bytes([bytes[0], bytes[1]]);
        if word == 0 {
            return None;
        }
        let digit = u64::from((word & 0x7fff).checked_sub(0x0100)?);
        value = value.checked_add(digit)?;
        cursor = cursor.checked_add(2)?;
        if word & 0x8000 != 0 {
            value = value.checked_mul(0x7f00)?;
        } else {
            return u32::try_from(value).ok();
        }
    }
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    let Some(bytes) = data.get(offset..).and_then(|tail| tail.get(..4)) else {
        return 0;
    };
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn f32_at(data: &[u8], offset: usize) -> f32 {
    f32::from_bits(u32_at(data, offset))
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{guessed_packet_size, header_name, write_decoded, write_hex, write_u16_hex};

    #[test]
    fn item_general_decoder_uses_documented_offsets() {
        let mut data = vec![0u8; 188];
        put_u32(&mut data, 0, 0x0161);
        put_u32(&mut data, 4, 101);
        put_u32(&mut data, 8, 0x8000_00CA);
        put_u32(&mut data, 12, 3);
        put_u32(&mut data, 16, 4);
        put_u32(&mut data, 20, 5);
        put_u32(&mut data, 24, 6);
        put_u32(&mut data, 28, 7);
        put_u32(&mut data, 32, 8);
        put_u32(&mut data, 36, 250);
        put_u32(&mut data, 40, 9090);
        put_u32(&mut data, 44, 11);
        put_u16(&mut data, 48, 0x21A8);
        put_u32(&mut data, 176, 1);
        data[180..184].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

        let decoded = decoded_fragment(0x0161, &data);

        assert_eq!(decoded["item_id"].as_u64(), Some(101));
        assert_eq!(decoded["model_file_id"].as_u64(), Some(202));
        assert_eq!(decoded["model_file_id_raw"].as_u64(), Some(0x8000_00CA));
        assert_eq!(decoded["item_type"].as_u64(), Some(3));
        assert_eq!(decoded["extra_id"].as_u64(), Some(5));
        assert_eq!(decoded["materials"].as_u64(), Some(6));
        assert_eq!(decoded["interaction"].as_u64(), Some(8));
        assert_eq!(decoded["price"].as_u64(), Some(250));
        assert_eq!(decoded["model_id"].as_u64(), Some(9090));
        assert_eq!(decoded["quantity"].as_u64(), Some(11));
        assert_eq!(decoded["name_id"].as_u64(), Some(8360));
        assert_eq!(decoded["enc_name_hex"].as_str(), Some("a8210000"));
        assert_eq!(decoded["mod_struct_size"].as_u64(), Some(1));
        assert_eq!(decoded["mods_hex"].as_str(), Some("deadbeef"));
    }

    #[test]
    fn item_general_decoder_matches_live_name_samples() {
        let salvage = decoded_fragment(
            0x0161,
            &item_general_sample(
                44,
                2_147_562_877,
                29,
                553_648_641,
                28,
                2992,
                1,
                &[0x1D, 0x27, 0x08, 0xAF, 0xC6, 0xBF, 0xED, 0x67, 0, 0],
                &[0, 3, 0, 0],
            ),
        );
        assert_eq!(salvage["model_file_id"].as_u64(), Some(79229));
        assert_eq!(salvage["model_id"].as_u64(), Some(2992));
        assert_eq!(salvage["name_id"].as_u64(), Some(9757));
        assert_eq!(
            salvage["enc_name_hex"].as_str(),
            Some("1d2708afc6bfed670000")
        );
        assert_eq!(salvage["mods_hex"].as_str(), Some("00030000"));

        let dust = decoded_fragment(
            0x0161,
            &item_general_sample(
                56,
                2_147_570_169,
                11,
                537_395_745,
                3,
                929,
                48,
                &[0xD8, 0x22, 0xA4, 0xA4, 0x0D, 0xED, 0x04, 0x43, 0, 0],
                &[0, 0, 0, 0],
            ),
        );
        assert_eq!(dust["model_file_id"].as_u64(), Some(86521));
        assert_eq!(dust["model_id"].as_u64(), Some(929));
        assert_eq!(dust["name_id"].as_u64(), Some(8664));
        assert_eq!(dust["enc_name_hex"].as_str(), Some("d822a4a40ded04430000"));
        assert_eq!(dust["mods_hex"].as_str(), Some("00000000"));
    }

    #[test]
    fn item_reuse_uses_item_general_capture_window() {
        let mut data = vec![0u8; 48];
        put_u32(&mut data, 0, 0x0162);
        put_u32(&mut data, 4, 303);
        put_u32(&mut data, 8, 404);

        let decoded = decoded_fragment(0x0162, &data);

        assert_eq!(header_name(0x0161), "ITEM_GENERAL_INFO");
        assert_eq!(header_name(0x0162), "ITEM_REUSE_ID");
        assert_eq!(guessed_packet_size(0x0162), 0x1b4);
        assert_eq!(decoded["item_id"].as_u64(), Some(303));
        assert_eq!(decoded["model_file_id"].as_u64(), Some(404));
    }

    #[test]
    fn hex_writers_cross_internal_chunk_boundaries() {
        let mut byte_hex = Vec::new();
        write_hex(&mut byte_hex, &vec![0xab; 257]).unwrap();
        assert_eq!(byte_hex, b"ab".repeat(257));

        let mut word_hex = Vec::new();
        write_u16_hex(&mut word_hex, &vec![0x1234; 129]).unwrap();
        assert_eq!(word_hex, b"3412".repeat(129));
    }

    #[expect(clippy::too_many_arguments, reason = "compact packet fixture")]
    fn item_general_sample(
        item_id: u32,
        model_file_id_raw: u32,
        item_type: u32,
        interaction: u32,
        price: u32,
        model_id: u32,
        quantity: u32,
        enc_name: &[u8],
        mods: &[u8],
    ) -> Vec<u8> {
        let mut data = vec![0u8; 188];
        put_u32(&mut data, 0, 0x0161);
        put_u32(&mut data, 4, item_id);
        put_u32(&mut data, 8, model_file_id_raw);
        put_u32(&mut data, 12, item_type);
        put_u32(&mut data, 32, interaction);
        put_u32(&mut data, 36, price);
        put_u32(&mut data, 40, model_id);
        put_u32(&mut data, 44, quantity);
        data[48..48 + enc_name.len()].copy_from_slice(enc_name);
        assert_eq!(mods.len() % size_of::<u32>(), 0);
        put_u32(&mut data, 176, (mods.len() / size_of::<u32>()) as u32);
        data[180..180 + mods.len()].copy_from_slice(mods);
        data
    }
    fn decoded_fragment(header: u32, data: &[u8]) -> Value {
        let mut json = Vec::new();
        assert!(write_decoded(&mut json, header, data).unwrap());
        serde_json::from_slice(&json).unwrap()
    }

    fn put_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
}
