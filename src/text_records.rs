use anyhow::{Context, bail};
use std::collections::BTreeMap;

pub(crate) const CLIENT_TEXT_FILE_ID_TABLE_VA: u32 = 0x00BEF1B8;
pub(crate) const CLIENT_TEXT_FILES_PER_LANGUAGE: usize = 99;
pub(crate) const TEXT_RECORDS_PER_FILE: u32 = 1024;
pub(crate) const CLIENT_LANGUAGE_CODES: &[(usize, &str)] = &[
    (0, "en"),
    (1, "ko"),
    (2, "fr"),
    (3, "de"),
    (4, "it"),
    (5, "es"),
    (6, "zh_tw"),
    (7, "zh_cn"),
    (8, "ja"),
    (9, "pl"),
    (10, "ru"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextRecord {
    pub(crate) record_start: usize,
    pub(crate) record_size: usize,
    pub(crate) compression_or_flags: u16,
    pub(crate) record_type: u8,
    pub(crate) record_subtype: u8,
    pub(crate) ordinal: u32,
    pub(crate) record_index: u32,
    pub(crate) text: String,
}

pub(crate) fn plausible_record_header(data: &[u8], offset: usize) -> bool {
    let Some(header) = data.get(offset..).and_then(|tail| tail.get(..6)) else {
        return false;
    };
    let size = u16::from_le_bytes([header[0], header[1]]) as usize;
    (6..=4096).contains(&size)
        && data
            .get(offset..)
            .and_then(|tail| tail.get(..size))
            .is_some()
}

#[cfg(test)]
pub(crate) fn parse_text_record_map(bytes: &[u8]) -> anyhow::Result<BTreeMap<u32, String>> {
    parse_text_record_map_with_decoded_records(bytes, &BTreeMap::new())
}

#[cfg(test)]
pub(crate) fn parse_text_record_map_with_decoded_records(
    bytes: &[u8],
    decoded_records: &BTreeMap<Vec<u8>, String>,
) -> anyhow::Result<BTreeMap<u32, String>> {
    parse_text_record_map_with_decoded_records_and_seeds(bytes, decoded_records, &BTreeMap::new())
}

pub(crate) fn parse_text_record_map_with_decoded_records_and_seeds(
    bytes: &[u8],
    decoded_records: &BTreeMap<Vec<u8>, String>,
    compact_seeds: &BTreeMap<u32, u64>,
) -> anyhow::Result<BTreeMap<u32, String>> {
    Ok(parse_text_record_entries_with_decoded_records_and_seeds(
        bytes,
        decoded_records,
        compact_seeds,
    )?
    .into_iter()
    .map(|record| (record.record_index, record.text))
    .collect())
}

pub(crate) fn parse_text_record_entries(bytes: &[u8]) -> anyhow::Result<Vec<TextRecord>> {
    parse_text_record_entries_with_decoded_records_and_seeds(
        bytes,
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
}

fn parse_text_record_entries_with_decoded_records_and_seeds(
    bytes: &[u8],
    decoded_records: &BTreeMap<Vec<u8>, String>,
    compact_seeds: &BTreeMap<u32, u64>,
) -> anyhow::Result<Vec<TextRecord>> {
    let mut records = Vec::new();
    if bytes.len() < 6 {
        return Ok(records);
    }

    let mut offset = find_record_start(bytes)?;
    let mut record_index = 0_u32;
    let mut text_ordinal = 0_u32;

    while plausible_record_header(bytes, offset) {
        let size = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        let compression_or_flags = u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]);
        let record_type = bytes[offset + 4];
        let record_subtype = bytes[offset + 5];
        let record_bytes = &bytes[offset..offset + size];

        let mut counts_as_text = false;
        let text = if let Some(text) = decoded_records.get(record_bytes) {
            counts_as_text = true;
            Some(text.clone())
        } else if record_type == 0x10 && record_subtype == 0 && compression_or_flags == 0 {
            counts_as_text = true;
            let payload = &bytes[offset + 6..offset + size];
            decode_record_payload(payload, compression_or_flags)
                .and_then(|decoded_bytes| decode_utf16le(&decoded_bytes))
                .ok()
        } else if record_subtype == 0 {
            compact_seeds.get(&record_index).and_then(|seed| {
                counts_as_text = true;
                decode_compact_record(record_bytes, *seed).ok()
            })
        } else {
            None
        };

        if let Some(text) = text {
            records.push(TextRecord {
                record_start: offset,
                record_size: size,
                compression_or_flags,
                record_type,
                record_subtype,
                ordinal: text_ordinal,
                record_index,
                text,
            });
        }
        if counts_as_text {
            text_ordinal += 1;
        }

        offset += size;
        record_index += 1;
    }

    Ok(records)
}

fn find_record_start(bytes: &[u8]) -> anyhow::Result<usize> {
    if plausible_record_header(bytes, 0) {
        return Ok(0);
    }

    for offset in 1..bytes.len().min(65536) {
        if plausible_record_header(bytes, offset) {
            let size = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) as usize;
            if offset
                .checked_add(size)
                .is_some_and(|next| plausible_record_header(bytes, next))
            {
                return Ok(offset);
            }
        }
    }

    bail!("text resource record header not found")
}

fn decode_record_payload(payload: &[u8], compression_or_flags: u16) -> anyhow::Result<Vec<u8>> {
    if compression_or_flags == 0 {
        return Ok(payload.to_vec());
    }

    let mut input = Vec::with_capacity(payload.len() + 7);
    input.extend_from_slice(payload);
    while input.len() % 4 != 0 {
        input.push(0);
    }
    input.extend_from_slice(&(u32::from(compression_or_flags)).to_le_bytes());
    crate::gw_dat_decompress::decompress_gw_dat(&input)
}

const COMPACT_SPECIAL: [u16; 32] = [
    0x0000, 0x0030, 0x0031, 0x0032, 0x0033, 0x0034, 0x0035, 0x0036, 0x0073, 0x0074, 0x0072, 0x006e,
    0x0075, 0x006d, 0x0028, 0x0029, 0x005b, 0x005d, 0x003c, 0x003e, 0x0025, 0x0023, 0x002f, 0x003a,
    0x002d, 0x0027, 0x0022, 0x0020, 0x002c, 0x002e, 0x0021, 0x000a,
];

fn decode_compact_record(record: &[u8], seed: u64) -> anyhow::Result<String> {
    let size = u16::from_le_bytes([record[0], record[1]]) as usize;
    let base = u16::from_le_bytes([record[2], record[3]]);
    let width = record[4];
    if size < 6 || size > record.len() || !(1..=16).contains(&width) {
        bail!("invalid compact text record header");
    }
    let payload = if seed == 0 {
        record[6..size].to_vec()
    } else {
        rc4_xor(&compact_rc4_key(seed), &record[6..size])
    };
    decode_compact_symbols(&payload, width, base)
}

fn decode_compact_symbols(payload: &[u8], width: u8, base: u16) -> anyhow::Result<String> {
    let mut bitbuf = 0_u32;
    let mut bitcount = 0_u32;
    let mut cursor = 0_usize;
    let mask = (1_u32 << width) - 1;
    let mut units = Vec::with_capacity((payload.len() * 8 / width as usize) + 1);

    for _ in 0..=(payload.len() * 8 / width as usize) {
        while bitcount <= 24 {
            if let Some(byte) = payload.get(cursor) {
                bitbuf |= u32::from(*byte) << bitcount;
                cursor += 1;
            }
            bitcount += 8;
        }
        let symbol = bitbuf & mask;
        bitbuf >>= width;
        bitcount -= u32::from(width);
        if symbol == 0 {
            break;
        }
        let unit = if symbol < 0x20 {
            COMPACT_SPECIAL[symbol as usize]
        } else {
            base.wrapping_add(symbol as u16).wrapping_sub(0x20)
        };
        units.push(unit);
    }

    String::from_utf16(&units).context("invalid compact UTF-16 text record")
}

fn compact_rc4_key(seed: u64) -> [u8; 20] {
    let seed = seed.to_le_bytes();
    let mut out = [0_u8; 20];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = seed[index % seed.len()];
    }

    let w0 = u32::from_le_bytes(out[0..4].try_into().unwrap());
    let w1 = u32::from_le_bytes(out[4..8].try_into().unwrap());
    let w2 = u32::from_le_bytes(out[8..12].try_into().unwrap());
    let w3 = u32::from_le_bytes(out[12..16].try_into().unwrap());
    let w4 = u32::from_le_bytes(out[16..20].try_into().unwrap());

    let a = w0.wrapping_add(0x9fb4_98b3);
    let b = w1.wrapping_add(0x66b0_cd0d).wrapping_add(a.rotate_left(5));
    let c = b
        .rotate_left(5)
        .wrapping_add(w2)
        .wrapping_add((!(a & 0x2222_2222) & 0x7bf3_6ae2).wrapping_add(0xf33d_5697));
    let a30 = a.rotate_left(30);
    let b30 = b.rotate_left(30);
    let d = w3
        .wrapping_add(c.rotate_left(5))
        .wrapping_add(((a30 ^ 0x59d1_48c0) & b) ^ 0x59d1_48c0)
        .wrapping_add(0xd675_e47b);
    let c30 = c.rotate_left(30);

    let words = [
        (((a30 ^ b30) & c) ^ a30)
            .wrapping_add(w4)
            .wrapping_add(d.rotate_left(5))
            .wrapping_add(0xb453_c259)
            .wrapping_add(w0),
        w1.wrapping_add(d),
        w2.wrapping_add(c30),
        w3.wrapping_add(b30),
        w4.wrapping_add(a30),
    ];
    for (chunk, word) in out.chunks_exact_mut(4).zip(words) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    out
}

fn rc4_xor(key: &[u8], input: &[u8]) -> Vec<u8> {
    let mut state = [0_u8; 256];
    for (index, byte) in state.iter_mut().enumerate() {
        *byte = index as u8;
    }
    let mut j = 0_u8;
    for index in 0..256 {
        j = j
            .wrapping_add(state[index])
            .wrapping_add(key[index % key.len()]);
        state.swap(index, j as usize);
    }

    let mut i = 0_u8;
    j = 0;
    input
        .iter()
        .map(|byte| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(state[i as usize]);
            state.swap(i as usize, j as usize);
            let key_byte = state[state[i as usize].wrapping_add(state[j as usize]) as usize];
            byte ^ key_byte
        })
        .collect()
}

fn decode_utf16le(bytes: &[u8]) -> anyhow::Result<String> {
    let usable_len = bytes.len() - (bytes.len() % 2);
    let units = bytes[..usable_len]
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&units).context("invalid UTF-16LE payload")
}
