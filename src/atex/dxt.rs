use anyhow::Context;

use super::checked_rgba_len;

pub(super) fn decode_dxta_rgba(dxt: &[u8], width: usize, height: usize) -> anyhow::Result<Vec<u8>> {
    let rgba_len = checked_rgba_len(width, height, "DXTA")?;
    let mut rgba = vec![0_u8; rgba_len];
    let mut pos = 0;
    for block_y in (0..height).step_by(4) {
        for block_x in (0..width).step_by(4) {
            let block = dxt.get(pos..pos + 8).context("DXTA block underrun")?;
            pos += 8;
            let decoded = decode_dxt5_alpha_block(block);
            for y in 0..4 {
                for x in 0..4 {
                    if block_y + y >= height || block_x + x >= width {
                        continue;
                    }
                    let value = decoded[y * 4 + x];
                    let dst = ((block_y + y) * width + block_x + x) * 4;
                    rgba[dst..dst + 4].copy_from_slice(&[value, value, value, 255]);
                }
            }
        }
    }
    Ok(rgba)
}

pub(super) fn decode_dxt5_rgba(
    dxt: &[u8],
    width: usize,
    height: usize,
    premultiply: bool,
) -> anyhow::Result<Vec<u8>> {
    let rgba_len = checked_rgba_len(width, height, "DXT5")?;
    let mut rgba = vec![0_u8; rgba_len];
    let mut pos = 0;
    for block_y in (0..height).step_by(4) {
        for block_x in (0..width).step_by(4) {
            let block = dxt.get(pos..pos + 16).context("DXT5 block underrun")?;
            pos += 16;
            let decoded = decode_dxt5_block(block);
            for y in 0..4 {
                for x in 0..4 {
                    if block_y + y >= height || block_x + x >= width {
                        continue;
                    }
                    let (mut r, mut g, mut b, a) = decoded[y * 4 + x];
                    if premultiply {
                        r = ((u16::from(r) * u16::from(a)) / 255) as u8;
                        g = ((u16::from(g) * u16::from(a)) / 255) as u8;
                        b = ((u16::from(b) * u16::from(a)) / 255) as u8;
                    }
                    let dst = ((block_y + y) * width + block_x + x) * 4;
                    rgba[dst..dst + 4].copy_from_slice(&[r, g, b, a]);
                }
            }
        }
    }
    Ok(rgba)
}
pub(super) fn decode_dxt3_rgba(dxt: &[u8], width: usize, height: usize) -> anyhow::Result<Vec<u8>> {
    let rgba_len = checked_rgba_len(width, height, "DXT3")?;
    let mut rgba = vec![0_u8; rgba_len];
    let mut pos = 0;
    for block_y in (0..height).step_by(4) {
        for block_x in (0..width).step_by(4) {
            let block = dxt.get(pos..pos + 16).context("DXT3 block underrun")?;
            pos += 16;

            let color0 = u16::from_le_bytes([block[8], block[9]]);
            let color1 = u16::from_le_bytes([block[10], block[11]]);
            let color_bits = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);
            let mut color_table = [(0_u8, 0_u8, 0_u8); 4];
            color_table[0] = rgb565(color0);
            color_table[1] = rgb565(color1);
            color_table[2] = lerp_rgb(color_table[0], color_table[1], 2, 1, 3);
            color_table[3] = lerp_rgb(color_table[0], color_table[1], 1, 2, 3);

            for y in 0..4 {
                for x in 0..4 {
                    if block_y + y >= height || block_x + x >= width {
                        continue;
                    }
                    let i = y * 4 + x;
                    let alpha_byte = block[i / 2];
                    let alpha_nibble = if i % 2 == 0 {
                        alpha_byte & 0x0f
                    } else {
                        alpha_byte >> 4
                    };
                    let color_index = ((color_bits >> (2 * i)) & 3) as usize;
                    let (r, g, b) = color_table[color_index];
                    let a = (u16::from(alpha_nibble) * 17) as u8;
                    let dst = ((block_y + y) * width + block_x + x) * 4;
                    rgba[dst..dst + 4].copy_from_slice(&[r, g, b, a]);
                }
            }
        }
    }
    Ok(rgba)
}

pub(super) fn decode_dxt1_rgba(dxt: &[u8], width: usize, height: usize) -> anyhow::Result<Vec<u8>> {
    let rgba_len = checked_rgba_len(width, height, "DXT1")?;
    let mut rgba = vec![0_u8; rgba_len];
    let mut pos = 0;
    for block_y in (0..height).step_by(4) {
        for block_x in (0..width).step_by(4) {
            let block = dxt.get(pos..pos + 8).context("DXT1 block underrun")?;
            pos += 8;

            let color0 = u16::from_le_bytes([block[0], block[1]]);
            let color1 = u16::from_le_bytes([block[2], block[3]]);
            let color_bits = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);

            let mut color_table = [(0_u8, 0_u8, 0_u8, 255_u8); 4];
            let c0 = rgb565(color0);
            let c1 = rgb565(color1);
            color_table[0] = (c0.0, c0.1, c0.2, 255);
            color_table[1] = (c1.0, c1.1, c1.2, 255);

            if color0 > color1 {
                let c2 = lerp_rgb(c0, c1, 2, 1, 3);
                let c3 = lerp_rgb(c0, c1, 1, 2, 3);
                color_table[2] = (c2.0, c2.1, c2.2, 255);
                color_table[3] = (c3.0, c3.1, c3.2, 255);
            } else {
                let c2 = lerp_rgb(c0, c1, 1, 1, 2);
                color_table[2] = (c2.0, c2.1, c2.2, 255);
                color_table[3] = (0, 0, 0, 0); // Transparent black
            }

            for y in 0..4 {
                for x in 0..4 {
                    if block_y + y >= height || block_x + x >= width {
                        continue;
                    }
                    let i = y * 4 + x;
                    let color_index = ((color_bits >> (2 * i)) & 3) as usize;
                    let (r, g, b, a) = color_table[color_index];
                    let dst = ((block_y + y) * width + block_x + x) * 4;
                    rgba[dst..dst + 4].copy_from_slice(&[r, g, b, a]);
                }
            }
        }
    }
    Ok(rgba)
}

fn decode_dxt5_block(block: &[u8]) -> [(u8, u8, u8, u8); 16] {
    let alpha = decode_dxt5_alpha_block(block);
    let color0 = u16::from_le_bytes([block[8], block[9]]);
    let color1 = u16::from_le_bytes([block[10], block[11]]);
    let color_bits = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);
    let mut color_table = [(0_u8, 0_u8, 0_u8); 4];
    color_table[0] = rgb565(color0);
    color_table[1] = rgb565(color1);
    color_table[2] = lerp_rgb(color_table[0], color_table[1], 2, 1, 3);
    color_table[3] = lerp_rgb(color_table[0], color_table[1], 1, 2, 3);

    let mut out = [(0_u8, 0_u8, 0_u8, 0_u8); 16];
    for (i, px) in out.iter_mut().enumerate() {
        let color_index = ((color_bits >> (2 * i)) & 3) as usize;
        let (r, g, b) = color_table[color_index];
        *px = (r, g, b, alpha[i]);
    }
    out
}

fn decode_dxt5_alpha_block(block: &[u8]) -> [u8; 16] {
    let alpha0 = block[0];
    let alpha1 = block[1];
    let alpha_bits = u64::from_le_bytes([
        block[2], block[3], block[4], block[5], block[6], block[7], 0, 0,
    ]);
    let mut alpha_table = [0_u8; 8];
    alpha_table[0] = alpha0;
    alpha_table[1] = alpha1;
    if alpha0 > alpha1 {
        for i in 1..=6 {
            alpha_table[i + 1] =
                (((7 - i) as u16 * u16::from(alpha0) + i as u16 * u16::from(alpha1)) / 7) as u8;
        }
    } else {
        for i in 1..=4 {
            alpha_table[i + 1] =
                (((5 - i) as u16 * u16::from(alpha0) + i as u16 * u16::from(alpha1)) / 5) as u8;
        }
        alpha_table[6] = 0;
        alpha_table[7] = 255;
    }

    let mut out = [0_u8; 16];
    for (i, value) in out.iter_mut().enumerate() {
        let alpha_index = ((alpha_bits >> (3 * i)) & 7) as usize;
        *value = alpha_table[alpha_index];
    }
    out
}

fn rgb565(color: u16) -> (u8, u8, u8) {
    let r = ((u32::from(color >> 11) & 0x1f) * 255 / 31) as u8;
    let g = ((u32::from(color >> 5) & 0x3f) * 255 / 63) as u8;
    let b = ((u32::from(color) & 0x1f) * 255 / 31) as u8;
    (r, g, b)
}

fn lerp_rgb(
    left: (u8, u8, u8),
    right: (u8, u8, u8),
    left_weight: u16,
    right_weight: u16,
    divisor: u16,
) -> (u8, u8, u8) {
    (
        ((u16::from(left.0) * left_weight + u16::from(right.0) * right_weight) / divisor) as u8,
        ((u16::from(left.1) * left_weight + u16::from(right.1) * right_weight) / divisor) as u8,
        ((u16::from(left.2) * left_weight + u16::from(right.2) * right_weight) / divisor) as u8,
    )
}
