use anyhow::{Context, bail};

use super::{AtexDxtFormat, AtexHeader, dxt_block_count};

const IMAGE_FORMATS: [u32; 23] = [
    0x0B2, 0x12, 0x0B2, 0x72, 0x12, 0x12, 0x12, 0x100, 0x1A4, 0x1A4, 0x1A4, 0x104, 0x0A2, 0x78,
    0x400, 0x71, 0x0B1, 0x0B1, 0x0B1, 0x0B1, 0x0A1, 0x11, 0x201,
];

const HUFF_BITS: &[u8] = &[
    0x6, 0x10, 0x6, 0x0f, 0x6, 0x0e, 0x6, 0x0d, 0x6, 0x0c, 0x6, 0x0b, 0x6, 0x0a, 0x6, 0x9, 0x6,
    0x8, 0x6, 0x7, 0x6, 0x6, 0x6, 0x5, 0x6, 0x4, 0x6, 0x3, 0x6, 0x2, 0x6, 0x1, 0x2, 0x11, 0x2,
    0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11,
    0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0,
    0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1,
    0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0,
    0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1,
    0x0,
];

const HUFF_VALUE: &[u8] = &[
    0x10, 0x6, 0x0f, 0x6, 0x0e, 0x6, 0x0d, 0x6, 0x0c, 0x6, 0x0b, 0x6, 0x0a, 0x6, 0x9, 0x6, 0x8,
    0x6, 0x7, 0x6, 0x6, 0x6, 0x5, 0x6, 0x4, 0x6, 0x3, 0x6, 0x2, 0x6, 0x1, 0x2, 0x11, 0x2, 0x11,
    0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2,
    0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1,
    0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0,
    0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1,
    0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0,
];

pub(super) fn decompress_atex_to_dxt5(
    atex_bytes: &[u8],
    header: AtexHeader,
) -> anyhow::Result<Vec<u8>> {
    if atex_bytes.len() < 20 {
        bail!("ATEX payload too small");
    }

    let words = atex_bytes
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect::<Vec<_>>();
    let data_size = words[3] as usize;
    let compression_code = words[4];
    let block_count = dxt_block_count(header.width as usize, header.height as usize, "ATEX")?;
    let block_bytes = if matches!(header.format, AtexDxtFormat::Dxt1 | AtexDxtFormat::DxtA) {
        8
    } else {
        16
    };
    let dxt_len = block_count
        .checked_mul(block_bytes)
        .context("ATEX DXT byte count overflow")?;

    let image_format = header.format.image_format()? as usize;
    let format_flags = *IMAGE_FORMATS
        .get(image_format)
        .context("unsupported ATEX image format index")?;
    let alpha_data_size2 = usize::from(image_format == 0x14) * 2;
    let alpha_data_size = usize::from((format_flags & 640) != 0) * 2;
    let color_data_size = usize::from((format_flags & 528) != 0) * 2;
    let block_size = alpha_data_size2 + alpha_data_size + color_data_size;
    if block_size != 2 && block_size != 4 {
        bail!("unsupported ATEX block layout size {block_size}");
    }
    let swizzled_borders = compression_code & 0x10 != 0
        && header.width == 256
        && header.height == 256
        && matches!(image_format, 0x10 | 0x11);

    let end_word = (12 + data_size) / 4;
    if end_word > words.len() || data_size <= 8 {
        bail!("invalid ATEX compressed data range");
    }

    let mut bits = BitReader::new(&words, 5, end_word);
    let mut out = vec![
        0_u32;
        block_count
            .checked_mul(block_size)
            .context("ATEX block output overflow")?
    ];
    let bitmap_len = block_count.div_ceil(32);
    let mut dcmp1 = vec![0_u32; bitmap_len];
    let mut dcmp2 = vec![0_u32; bitmap_len];
    if swizzled_borders {
        subcode1(&mut dcmp1, &mut dcmp2, block_count);
    }

    if compression_code & 1 != 0 && header.format == AtexDxtFormat::Dxt1 {
        subcode2(
            &mut out,
            &mut dcmp1,
            &mut dcmp2,
            &mut bits,
            block_count,
            block_size,
        )?;
    }
    if compression_code & 2 != 0
        && matches!(
            header.format,
            AtexDxtFormat::Dxt2 | AtexDxtFormat::Dxt3 | AtexDxtFormat::DxtN
        )
    {
        subcode3(&mut out, &mut dcmp1, &mut bits, block_count, block_size)?;
    }
    if compression_code & 4 != 0
        && matches!(
            header.format,
            AtexDxtFormat::Dxt4 | AtexDxtFormat::Dxt5 | AtexDxtFormat::DxtA | AtexDxtFormat::DxtL
        )
    {
        subcode4(
            &mut out,
            &mut dcmp1,
            &dcmp2,
            &mut bits,
            block_count,
            block_size,
        )?;
    }
    if compression_code & 8 != 0 {
        subcode5(
            &mut out,
            &mut dcmp2,
            &mut bits,
            block_count,
            block_size,
            header.format == AtexDxtFormat::Dxt1,
        )?;
    }

    let mut pos = bits.pos.saturating_sub(1);
    if alpha_data_size > 0 || alpha_data_size2 > 0 {
        let raw_alpha_words = (0..block_count)
            .filter(|block| dcmp1[*block >> 5] & (1 << (*block & 31)) == 0)
            .count()
            * 2;
        let dxt3_opaque_alpha_fallback = matches!(
            header.format,
            AtexDxtFormat::Dxt2 | AtexDxtFormat::Dxt3 | AtexDxtFormat::DxtN
        ) && pos + raw_alpha_words > end_word;
        for block in 0..block_count {
            if dcmp1[block >> 5] & (1 << (block & 31)) == 0 {
                let dst = block * block_size;
                if dxt3_opaque_alpha_fallback {
                    out[dst] = u32::MAX;
                    out[dst + 1] = u32::MAX;
                } else {
                    out[dst] = *words.get(pos).context("ATEX alpha block underrun")?;
                    out[dst + 1] = *words.get(pos + 1).context("ATEX alpha block underrun")?;
                    pos += 2;
                }
            }
        }
    }
    if color_data_size > 0 {
        for block in 0..block_count {
            if dcmp2[block >> 5] & (1 << (block & 31)) == 0 {
                let dst = block * block_size + alpha_data_size2 + alpha_data_size;
                out[dst] = *words.get(pos).context("ATEX color block underrun")?;
                pos += 1;
            }
        }
        for block in 0..block_count {
            if dcmp2[block >> 5] & (1 << (block & 31)) == 0 {
                let dst = block * block_size + alpha_data_size2 + alpha_data_size + 1;
                out[dst] = *words.get(pos).context("ATEX color index block underrun")?;
                pos += 1;
            }
        }
    }

    if swizzled_borders {
        subcode7(&mut out, block_count, block_size)?;
    }

    let mut bytes = Vec::with_capacity(dxt_len);
    for word in out {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    Ok(bytes)
}

struct BitReader<'a> {
    words: &'a [u32],
    pos: usize,
    end: usize,
    remaining: u32,
    current: u32,
    next: u32,
}

impl<'a> BitReader<'a> {
    fn new(words: &'a [u32], pos: usize, end: usize) -> Self {
        let mut reader = Self {
            words,
            pos,
            end,
            remaining: 0,
            current: 0,
            next: 0,
        };
        if reader.pos != reader.end {
            reader.current = reader.words[reader.pos];
            reader.pos += 1;
        }
        reader
    }

    fn consume(&mut self, bits: u32) {
        if bits == 0 {
            return;
        }
        self.current = (self.current << bits) | (self.next >> (32 - bits));
        if bits > self.remaining {
            if self.pos != self.end {
                let word = self.words[self.pos];
                self.pos += 1;
                self.current |= word >> (self.remaining + 32 - bits);
                self.next = word << (bits - self.remaining);
                self.remaining += 32 - bits;
            } else {
                self.next = 0;
                self.remaining = 0;
            }
        } else {
            self.remaining -= bits;
            self.next <<= bits;
        }
    }

    fn bit(&mut self) -> u32 {
        let bit = self.current >> 31;
        self.current = (self.current << 1) | (self.next >> 31);
        if self.remaining > 0 {
            self.next <<= 1;
            self.remaining -= 1;
        } else if self.pos != self.end {
            let word = self.words[self.pos];
            self.pos += 1;
            self.current |= word >> 31;
            self.next = word << 1;
            self.remaining = 31;
        } else {
            self.next = 0;
            self.remaining = 0;
        }
        bit
    }

    fn take_bits(&mut self, bits: u32) -> u32 {
        if bits == 0 {
            return 0;
        }
        let value = if bits == 32 {
            self.current
        } else {
            self.current >> (32 - bits)
        };
        self.consume(bits);
        value
    }
}

fn subcode1(dcmp1: &mut [u32], dcmp2: &mut [u32], block_count: usize) {
    const BORDER_MASK: u32 = 0xc000_0003;
    for block in 0..block_count {
        let mask = 1_u32 << (block & 31);
        let row_mask = 1_u32 << ((block >> 6) & 31);
        if mask & BORDER_MASK != 0 || row_mask & BORDER_MASK != 0 {
            dcmp1[block >> 5] |= mask;
            dcmp2[block >> 5] |= mask;
        }
    }
}

fn subcode2(
    out: &mut [u32],
    dcmp1: &mut [u32],
    dcmp2: &mut [u32],
    bits: &mut BitReader<'_>,
    block_count: usize,
    block_size: usize,
) -> anyhow::Result<()> {
    let mut block = 0;
    while block < block_count {
        let huff = (bits.current >> 26) as usize;
        let read_count = HUFF_VALUE[huff * 2] as usize + 1;
        let shift = HUFF_BITS[huff * 2] as u32;
        bits.consume(shift);

        let fill_block = bits.bit() != 0;
        let mut remaining = read_count;
        while remaining > 0 && block < block_count {
            let mask = 1_u32 << (block & 31);
            let word = block >> 5;
            if dcmp2[word] & mask == 0 {
                if fill_block {
                    let dst = block * block_size;
                    out[dst] = 0xffff_fffe;
                    out[dst + 1] = 0xffff_ffff;
                    dcmp1[word] |= mask;
                    dcmp2[word] |= mask;
                }
                remaining -= 1;
            }
            block += 1;
        }
        while block < block_count {
            let mask = 1_u32 << (block & 31);
            if dcmp2[block >> 5] & mask == 0 {
                break;
            }
            block += 1;
        }
    }
    Ok(())
}

fn subcode3(
    out: &mut [u32],
    dcmp1: &mut [u32],
    bits: &mut BitReader<'_>,
    block_count: usize,
    block_size: usize,
) -> anyhow::Result<()> {
    let alpha_nibble = bits.take_bits(4);
    let alpha_byte = (alpha_nibble << 4) | alpha_nibble;
    let alpha_pattern = alpha_byte | (alpha_byte << 8) | (alpha_byte << 16) | (alpha_byte << 24);
    let alpha_table = [0, 0, 0, 0, alpha_pattern, alpha_pattern, 0, 0];

    let mut block = 0;
    while block < block_count {
        let huff = (bits.current >> 26) as usize;
        let read_count = HUFF_VALUE[huff * 2] as usize + 1;
        let shift = HUFF_BITS[huff * 2] as u32;
        bits.consume(shift);

        let flag1 = bits.bit();
        let alpha_index = if flag1 == 0 { 0 } else { flag1 + bits.bit() } as usize;
        let mut remaining = read_count;
        while remaining > 0 && block < block_count {
            let mask = 1_u32 << (block & 31);
            let word = block >> 5;
            if dcmp1[word] & mask == 0 {
                if alpha_index != 0 {
                    let dst = block * block_size;
                    out[dst] = alpha_table[alpha_index * 2];
                    out[dst + 1] = alpha_table[alpha_index * 2 + 1];
                    dcmp1[word] |= mask;
                }
                remaining -= 1;
            }
            block += 1;
        }
        while block < block_count {
            let mask = 1_u32 << (block & 31);
            if dcmp1[block >> 5] & mask == 0 {
                break;
            }
            block += 1;
        }
    }
    Ok(())
}

fn subcode4(
    out: &mut [u32],
    dcmp1: &mut [u32],
    dcmp2: &[u32],
    bits: &mut BitReader<'_>,
    block_count: usize,
    block_size: usize,
) -> anyhow::Result<()> {
    let extracted = (bits.current >> 24) & 0xff;
    bits.current = (bits.current << 8) | (bits.next >> 24);
    if bits.remaining < 8 {
        if bits.pos != bits.end {
            let word = bits.words[bits.pos];
            bits.pos += 1;
            bits.current |= word >> (bits.remaining + 24);
            bits.next = word << (8 - bits.remaining);
            bits.remaining += 24;
        } else {
            bits.next = 0;
            bits.remaining = 0;
        }
    } else {
        bits.next <<= 8;
        bits.remaining -= 8;
    }

    let color_pattern = (extracted << 8) | extracted;
    let color_table = [0, 0, 0, 0, color_pattern, 0, 0, 0];
    let mut block = 0;
    while block < block_count {
        let shift_index = (bits.current >> 26) as usize;
        let read_count = HUFF_VALUE[shift_index * 2] as usize + 1;
        let shift = HUFF_BITS[shift_index * 2] as u32;
        bits.consume(shift);

        let flag1 = bits.bit();
        let color_index = if flag1 == 0 { 0 } else { flag1 + bits.bit() } as usize;
        let mut remaining = read_count;
        while remaining > 0 && block < block_count {
            let mask = 1_u32 << (block & 31);
            let word = block >> 5;
            if dcmp2[word] & mask == 0 {
                if color_index != 0 {
                    let dst = block * block_size;
                    out[dst] = color_table[color_index * 2];
                    out[dst + 1] = color_table[color_index * 2 + 1];
                    dcmp1[word] |= mask;
                }
                remaining -= 1;
            }
            block += 1;
        }
        while block < block_count {
            let mask = 1_u32 << (block & 31);
            if dcmp2[block >> 5] & mask == 0 {
                break;
            }
            block += 1;
        }
    }
    Ok(())
}

fn subcode5(
    out: &mut [u32],
    dcmp2: &mut [u32],
    bits: &mut BitReader<'_>,
    block_count: usize,
    block_size: usize,
    dxt1_flag: bool,
) -> anyhow::Result<()> {
    let color_value = (bits.current >> 8) | 0xff00_0000;
    let mut new_current = (bits.current << 24) | (bits.next >> 8);
    if bits.remaining >= 24 {
        bits.next <<= 24;
        bits.remaining -= 24;
    } else if bits.pos != bits.end {
        let word = bits.words[bits.pos];
        bits.pos += 1;
        let shift = bits.remaining + 8;
        new_current |= word >> shift;
        bits.next = word << (24 - bits.remaining);
        bits.remaining = shift;
    } else {
        bits.next = 0;
        bits.remaining = 0;
    }
    bits.current = new_current;

    let color_data = subcode6(color_value, dxt1_flag);
    let mut block = 0;
    while block < block_count {
        let huff = ((bits.current >> 26) & 0x3f) as usize;
        let huff_bits = HUFF_BITS[huff * 2] as u32;
        let huff_value = HUFF_VALUE[huff * 2] as usize + 1;
        if huff_bits < 32 {
            bits.consume(huff_bits);
        }
        let bit = bits.bit();
        let mut remaining = huff_value;
        while remaining > 0 && block < block_count {
            let mask = 1_u32 << (block & 31);
            let word = block >> 5;
            if dcmp2[word] & mask == 0 {
                if bit != 0 {
                    let color_offset = if dxt1_flag { 0 } else { 2 };
                    let dst = block * block_size + color_offset;
                    out[dst] = color_data[0];
                    out[dst + 1] = color_data[1];
                    dcmp2[word] |= mask;
                }
                remaining -= 1;
            }
            block += 1;
        }
        while block < block_count {
            let mask = 1_u32 << (block & 31);
            if dcmp2[block >> 5] & mask == 0 {
                break;
            }
            block += 1;
        }
    }
    Ok(())
}

fn subcode6(color_value: u32, dxt1_flag: bool) -> [u32; 2] {
    let r = color_value & 0xff;
    let g = (color_value >> 8) & 0xff;
    let b = (color_value >> 16) & 0xff;

    let bases = [
        (r - (r >> 5)) >> 3,
        (g - (g >> 6)) >> 2,
        (b - (b >> 5)) >> 3,
    ];
    let quantized = [
        (bases[0] >> 2) + bases[0] * 8,
        (bases[1] >> 4) + bases[1] * 4,
        (bases[2] >> 2) + bases[2] * 8,
    ];
    let next_bases = [bases[0] + 1, bases[1] + 1, bases[2] + 1];
    let next_quantized = [
        (next_bases[0] >> 2) + next_bases[0] * 8,
        (next_bases[1] >> 4) + next_bases[1] * 4,
        (next_bases[2] >> 2) + next_bases[2] * 8,
    ];
    let channels = [r, g, b];
    let mut values = [0_u32; 3];
    for i in 0..3 {
        let delta = next_quantized[i] - quantized[i];
        values[i] = (channels[i] * 12 - quantized[i] * 12)
            .checked_div(delta)
            .unwrap_or(0);
    }

    let mut palette = [(0_u32, 0_u32); 3];
    for i in 0..3 {
        palette[i] = match values[i] {
            0..=1 => (bases[i], bases[i]),
            2..=5 => (bases[i], bases[i] + 1),
            6..=9 => (bases[i] + 1, bases[i]),
            _ => (bases[i] + 1, bases[i] + 1),
        };
    }

    let mut color1 = palette[0].0 | (palette[1].0 << 5) | (palette[2].0 << 11);
    let mut color2 = palette[0].1 | (palette[1].1 << 5) | (palette[2].1 << 11);
    let mut score = 0;
    let mut count = 0;
    for i in 0..3 {
        if palette[i].0 != palette[i].1 {
            score += if palette[i].0 == bases[i] {
                values[i]
            } else {
                12 - values[i]
            };
            count += 1;
        }
    }
    let mut avg = (score + count / 2).checked_div(count).unwrap_or(0);
    let swap = dxt1_flag && ((avg == 5 || avg == 6) || count == 0);
    if count == 0 && !swap {
        if color2 != 0xffff {
            avg = 0;
            color2 += 1;
        } else {
            avg = 12;
            color1 -= 1;
        }
    }
    if (color1 < color2) != swap {
        std::mem::swap(&mut color1, &mut color2);
        avg = 12 - avg;
    }

    let table = if swap {
        2
    } else if avg < 2 {
        0
    } else if avg < 6 {
        2
    } else if avg < 10 {
        3
    } else {
        1
    };
    let mut pattern = table * 5;
    pattern = (pattern << 4) | pattern;
    pattern = (pattern << 8) | pattern;
    pattern = (pattern << 16) | pattern;
    [(color2 << 16) | color1, pattern]
}

fn subcode7(out: &mut [u32], block_count: usize, block_size: usize) -> anyhow::Result<()> {
    if block_size != 4 {
        bail!("ATEX border unswizzle requires four words per block");
    }
    const BORDER_MASK: u32 = 0xc000_0003;
    for block in 0..block_count {
        let position_in_row = block & 0x3f;
        let row = block >> 6;
        let low_swizzle = (1_u32 << (position_in_row & 31)) & BORDER_MASK != 0;
        let high_swizzle = (1_u32 << (row & 31)) & BORDER_MASK != 0;
        if !low_swizzle && !high_swizzle {
            continue;
        }

        let source_in_row = if low_swizzle {
            position_in_row ^ 3
        } else {
            position_in_row
        };
        let source_row = if high_swizzle { row ^ 3 } else { row };
        let source = (source_row << 6) + source_in_row;
        if source >= block_count {
            continue;
        }
        let source_offset = source * block_size;
        let source_words = out
            .get(source_offset..source_offset + 4)
            .context("ATEX border source block out of bounds")?;
        let [mut data0, mut data1, data2, mut data3] =
            <[u32; 4]>::try_from(source_words).expect("four-word slice");

        if low_swizzle {
            for _ in 0..2 {
                let mixed_high = ((data0 >> 8) & 0x00f0_00f0) | (data0 & 0x0f00_0f00);
                let mixed_low = ((data0 & 0xffff_000f) << 8) | (data0 & 0x00f0_00f0);
                data0 = (mixed_high >> 4) | (mixed_low << 4);
            }
            let low = ((data3 & 0xff03_0303) << 4) | (data3 & 0x0c0c_0c0c);
            let high = ((data3 >> 4) & 0x0c0c_0c0c) | (data3 & 0x3030_3030);
            data3 = (low << 2) | (high >> 2);
        }
        if high_swizzle {
            let old_data0 = data0;
            data0 = data1.rotate_left(16);
            data1 = old_data0.rotate_left(16);
            let low = (data3 & 0x00ff_0000) | (data3 >> 16);
            let high = (data3 << 16) | (data3 & 0x0000_ff00);
            data3 = (low >> 8) | (high << 8);
        }

        let destination = block * block_size;
        out[destination..destination + 4].copy_from_slice(&[data0, data1, data2, data3]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subcode2_marks_dxt1_constant_blocks() {
        let words = [0xffff_ffff_u32];
        let mut bits = BitReader {
            words: &words,
            pos: 1,
            end: 1,
            remaining: 0,
            current: words[0],
            next: 0,
        };
        let mut out = vec![0_u32; 2];
        let mut dcmp1 = vec![0_u32; 1];
        let mut dcmp2 = vec![0_u32; 1];

        subcode2(&mut out, &mut dcmp1, &mut dcmp2, &mut bits, 1, 2).unwrap();

        assert_eq!(out, [0xffff_fffe, 0xffff_ffff]);
        assert_eq!(dcmp1[0], 1);
        assert_eq!(dcmp2[0], 1);
    }

    #[test]
    fn border_subcodes_mark_and_remap_swizzled_dxt3_blocks() {
        let mut dcmp1 = [0_u32; 8];
        let mut dcmp2 = [0_u32; 8];
        subcode1(&mut dcmp1, &mut dcmp2, 256);
        assert_ne!(dcmp1[128 >> 5] & (1 << (128 & 31)), 0);
        assert_eq!(dcmp1[130 >> 5] & (1 << (130 & 31)), 0);
        assert_ne!(dcmp2[158 >> 5] & (1 << (158 & 31)), 0);

        let mut out = (0..256_u32)
            .flat_map(|block| [block, block + 1_000, block + 2_000, block + 3_000])
            .collect::<Vec<_>>();
        subcode7(&mut out, 256, 4).unwrap();

        assert_eq!(out[128 * 4 + 2], 131 + 2_000);
        assert_eq!(out[130 * 4 + 2], 130 + 2_000);
    }
}
