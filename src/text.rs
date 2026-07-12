pub(crate) fn decompress_and_decode_entry_records(data: &[u8]) -> Vec<u8> {
    // Check if the data starts with a plausible record header:
    if data.len() < 6 {
        return data.to_vec();
    }
    let first_size = u16::from_le_bytes([data[0], data[1]]) as usize;
    if first_size < 6 || first_size > data.len() {
        return data.to_vec();
    }

    let mut decompressed_data = Vec::new();
    let mut offset = 0;
    while offset + 6 <= data.len() {
        let size = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        if size < 6 || offset + size > data.len() {
            break;
        }
        let compression_or_flags =
            u16::from_le_bytes([data[offset + 2], data[offset + 3]]) as usize;
        let record_type = data[offset + 4];
        let record_subtype = data[offset + 5];

        if record_type == 0x10 && record_subtype == 0 {
            let payload = &data[offset + 6..offset + size];
            if compression_or_flags == 0 {
                decompressed_data.extend_from_slice(payload);
            } else {
                // Decompress record payload: payload + padding + out_size
                let mut input = Vec::new();
                input.extend_from_slice(payload);
                while input.len() % 4 != 0 {
                    input.push(0);
                }
                input.extend_from_slice(&(compression_or_flags as u32).to_le_bytes());
                if let Ok(dec) = crate::gw_dat_decompress::decompress_gw_dat(&input) {
                    decompressed_data.extend_from_slice(&dec);
                }
            }
            // Separator matching looks_like_gw_string_trailer: \x00\x00\x10\x00
            decompressed_data.extend_from_slice(&[0x00, 0x00, 0x10, 0x00]);
        }
        offset += size;
    }

    if decompressed_data.is_empty() {
        data.to_vec()
    } else {
        decompressed_data
    }
}

pub(crate) fn utf16le_strings(bytes: &[u8]) -> Vec<(usize, String)> {
    let decompressed = decompress_and_decode_entry_records(bytes);
    utf16le_strings_with_min_len(&decompressed, 4)
}

pub(crate) fn utf16le_strings_with_min_len(bytes: &[u8], min_len: usize) -> Vec<(usize, String)> {
    let mut strings = Vec::new();

    for alignment in 0..=1 {
        if bytes.len() <= alignment {
            continue;
        }

        let mut current = String::new();
        let mut start = alignment;

        for (index, chunk) in bytes[alignment..].chunks_exact(2).enumerate() {
            let offset = alignment + index * 2;
            let code = u16::from_le_bytes([chunk[0], chunk[1]]);
            let Some(ch) = char::from_u32(u32::from(code)) else {
                continue;
            };

            if ch.is_ascii_graphic() || ch == ' ' || ch == '\t' || ch == '\n' {
                if !current.is_empty() && looks_like_gw_string_trailer(bytes, offset) {
                    if current.len() >= min_len {
                        strings.push((start, std::mem::take(&mut current)));
                    } else {
                        current.clear();
                    }
                    continue;
                }

                if current.is_empty() {
                    start = offset;
                }
                current.push(ch);
            } else if current.len() >= min_len {
                strings.push((start, std::mem::take(&mut current)));
            } else {
                current.clear();
            }
        }

        if current.len() >= min_len {
            strings.push((start, current));
        }
    }

    strings.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    strings.dedup();
    strings
}

pub(crate) fn looks_like_gw_string_trailer(bytes: &[u8], offset: usize) -> bool {
    bytes.get(offset + 2..offset + 6) == Some(&[0x00, 0x00, 0x10, 0x00])
}

pub(crate) fn ascii_preview(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut current = String::new();
    for &byte in bytes.iter().take(4096) {
        if byte.is_ascii_graphic() || byte == b' ' {
            current.push(byte as char);
        } else {
            if current.len() >= 4 {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(&current);
                if out.len() >= 240 {
                    break;
                }
            }
            current.clear();
        }
    }
    if current.len() >= 4 && out.len() < 240 {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&current);
    }
    out.truncate(240);
    out
}

pub(crate) fn utf16le_preview(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut current = String::new();
    for chunk in bytes.chunks_exact(2).take(2048) {
        let code = u16::from_le_bytes([chunk[0], chunk[1]]);
        let Some(ch) = char::from_u32(u32::from(code)) else {
            continue;
        };
        if ch.is_ascii_graphic() || ch == ' ' {
            current.push(ch);
        } else {
            if current.len() >= 4 {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(&current);
                if out.len() >= 240 {
                    break;
                }
            }
            current.clear();
        }
    }
    if current.len() >= 4 && out.len() < 240 {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&current);
    }
    out.truncate(240);
    out
}

pub(crate) fn detect_entry_kind(bytes: &[u8]) -> &'static str {
    if let Some(kind) = crate::atex::detect_kind(bytes) {
        kind
    } else if bytes.starts_with(b";===") || bytes.starts_with(b";***") {
        "text_resource"
    } else if bytes.starts_with(b"ffna") {
        "ffna"
    } else if bytes.starts_with(b"DDS ") {
        "dds_texture"
    } else if bytes.starts_with(b"AMAT") {
        "amat_material"
    } else if bytes.starts_with(b"AMP")
        || bytes.starts_with(b"ID3")
        || bytes
            .get(..2)
            .is_some_and(|prefix| matches!(prefix, [0xff, 0xfa] | [0xff, 0xfb]))
    {
        "sound"
    } else if !utf16le_preview(bytes).is_empty() {
        "text_utf16le"
    } else if !ascii_preview(bytes).is_empty() {
        "text_or_binary_ascii"
    } else {
        "unknown"
    }
}

pub(crate) fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks_exact(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

pub(crate) fn encoded_words_from_hex(hex: &str) -> Option<Vec<u16>> {
    let bytes = hex_to_bytes(hex)?;
    let mut words = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        let word = u16::from_le_bytes([chunk[0], chunk[1]]);
        if word == 0 {
            break;
        }
        words.push(word);
    }
    Some(words)
}

pub(crate) fn encoded_values_from_hex(hex: &str) -> Option<Vec<(u64, usize, usize)>> {
    encoded_values_from_words(&encoded_words_from_hex(hex)?)
}

pub(crate) fn encoded_values_from_words(words: &[u16]) -> Option<Vec<(u64, usize, usize)>> {
    let mut values = Vec::new();
    let mut cursor = 0;
    while cursor < words.len() {
        if words[cursor] < 0x0100 {
            cursor += 1;
            continue;
        }
        let start = cursor;
        let (value, consumed) = encoded_u64_from_words(&words[cursor..])?;
        cursor += consumed;
        values.push((value, start, cursor));
    }
    Some(values)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TextReference {
    pub(crate) id: u32,
    pub(crate) seed: u64,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

pub(crate) fn text_references(words: &[u16]) -> Vec<TextReference> {
    let Some(values) = encoded_values_from_words(words) else {
        return Vec::new();
    };
    let mut references = Vec::new();
    let mut index = 0;
    while index < values.len() {
        let (id, start, end) = values[index];
        if let Ok(id) = u32::try_from(id) {
            if let Some(&(seed, _, seed_end)) = values.get(index + 1)
                && seed > u64::from(u32::MAX)
            {
                references.push(TextReference {
                    id,
                    seed,
                    start,
                    end: seed_end,
                });
                index += 2;
                continue;
            }
            if id >= 1024 && words.get(end) == Some(&0x010a) {
                references.push(TextReference {
                    id,
                    seed: 0,
                    start,
                    end,
                });
            }
        }
        index += 1;
    }
    references
}

fn encoded_u64_from_words(words: &[u16]) -> Option<(u64, usize)> {
    let mut value = 0_u64;
    for (index, &word) in words.iter().enumerate() {
        let digit = u64::from((word & 0x7fff).checked_sub(0x0100)?);
        value = value.checked_add(digit)?;
        if word & 0x8000 != 0 {
            value = value.checked_mul(0x7f00)?;
        } else {
            return Some((value, index + 1));
        }
    }
    None
}

pub(crate) fn apply_encoded_template(template: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut cursor = 0;
    let mut next_unnumbered = 0;
    while let Some(relative_start) = template[cursor..].find('%') {
        let start = cursor + relative_start;
        out.push_str(&template[cursor..start]);
        let rest = &template[start + 1..];
        let kind_len = ["str", "num", "s", "d"]
            .into_iter()
            .find(|kind| rest.starts_with(kind))
            .map_or(0, str::len);
        let digits_start = start + 1 + kind_len;
        let digits_len = template[digits_start..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        if kind_len == 0 && digits_len == 0 {
            out.push('%');
            cursor = start + 1;
            continue;
        }
        let end = digits_start + digits_len;
        let arg_index = if digits_len == 0 {
            let index = next_unnumbered;
            next_unnumbered += 1;
            Some(index)
        } else {
            template[digits_start..end]
                .parse::<usize>()
                .ok()
                .and_then(|index| index.checked_sub(1))
        };
        if let Some(arg) = arg_index.and_then(|index| args.get(index)) {
            out.push_str(arg);
        } else {
            out.push_str(&template[start..end]);
        }
        cursor = end;
    }
    out.push_str(&template[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::apply_encoded_template;

    #[test]
    fn replaces_unnumbered_placeholders_in_textual_order() {
        let args = ["first", "second"].map(str::to_string);
        assert_eq!(
            apply_encoded_template("%num puis %str", &args),
            "first puis second"
        );
    }

    #[test]
    fn parses_complete_numbered_placeholder_tokens() {
        let args = (1..=10)
            .map(|index| format!("arg{index}"))
            .collect::<Vec<_>>();
        assert_eq!(
            apply_encoded_template("%str10 / %str1 / %12", &args),
            "arg10 / arg1 / %12"
        );
    }
}
