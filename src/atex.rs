use crate::io_util::read_u32;
use anyhow::{Context, bail};
use image::{ColorType, ImageBuffer, ImageFormat, Rgba, save_buffer_with_format};
use std::path::Path;

const MAX_TEXTURE_PIXELS: usize = 8192 * 8192;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtexContainer {
    Atex,
    Attx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtexDxtFormat {
    Dxt1,
    Dxt2,
    Dxt3,
    Dxt4,
    Dxt5,
    DxtN,
    DxtA,
    DxtL,
}

impl AtexDxtFormat {
    pub(crate) fn from_fourcc(fourcc: [u8; 4]) -> anyhow::Result<Self> {
        match &fourcc {
            b"DXT1" => Ok(Self::Dxt1),
            b"DXT2" => Ok(Self::Dxt2),
            b"DXT3" => Ok(Self::Dxt3),
            b"DXT4" => Ok(Self::Dxt4),
            b"DXT5" => Ok(Self::Dxt5),
            b"DXTN" => Ok(Self::DxtN),
            b"DXTA" => Ok(Self::DxtA),
            b"DXTL" => Ok(Self::DxtL),
            _ => bail!(
                "unsupported ATEX DXT FourCC {}",
                String::from_utf8_lossy(&fourcc)
            ),
        }
    }

    pub(crate) fn as_fourcc(self) -> &'static str {
        match self {
            Self::Dxt1 => "DXT1",
            Self::Dxt2 => "DXT2",
            Self::Dxt3 => "DXT3",
            Self::Dxt4 => "DXT4",
            Self::Dxt5 => "DXT5",
            Self::DxtN => "DXTN",
            Self::DxtA => "DXTA",
            Self::DxtL => "DXTL",
        }
    }

    #[cfg(test)]
    pub(crate) fn dds_fourcc(self) -> &'static str {
        match self {
            Self::Dxt1 => "DXT1",
            Self::Dxt2 | Self::Dxt3 | Self::DxtN => "DXT3",
            Self::Dxt4 | Self::Dxt5 | Self::DxtA | Self::DxtL => "DXT5",
        }
    }

    fn image_format(self) -> anyhow::Result<u32> {
        match self {
            Self::Dxt1 => Ok(0x0f),
            Self::Dxt2 | Self::Dxt3 | Self::DxtN => Ok(0x11),
            Self::Dxt4 | Self::Dxt5 | Self::DxtA => Ok(0x13),
            Self::DxtL => Ok(0x12),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AtexHeader {
    pub(crate) container: AtexContainer,
    pub(crate) format: AtexDxtFormat,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

pub(crate) fn parse_header(bytes: &[u8]) -> anyhow::Result<AtexHeader> {
    let header = bytes
        .get(..12)
        .context("ATEX header shorter than 12 bytes")?;

    let container = match &header[0..4] {
        b"ATEX" => AtexContainer::Atex,
        b"ATTX" => AtexContainer::Attx,
        other => bail!(
            "not an ATEX/ATTX texture: {}",
            String::from_utf8_lossy(other)
        ),
    };

    let format = AtexDxtFormat::from_fourcc([header[4], header[5], header[6], header[7]])?;
    let width = u16::from_le_bytes([header[8], header[9]]);
    let height = u16::from_le_bytes([header[10], header[11]]);
    if width == 0 || height == 0 {
        bail!("ATEX texture has zero dimensions: {width}x{height}");
    }

    Ok(AtexHeader {
        container,
        format,
        width,
        height,
    })
}

pub(crate) fn detect_kind(bytes: &[u8]) -> Option<&'static str> {
    let header = parse_header(bytes).ok()?;
    match header.container {
        AtexContainer::Atex => Some("atex_texture"),
        AtexContainer::Attx => Some("attx_texture"),
    }
}

pub(crate) fn save_atex_as_png(atex_bytes: &[u8], out_path: &Path) -> anyhow::Result<()> {
    save_atex_as_png_with_alpha_policy(atex_bytes, out_path, AlphaPolicy::ForceOpaque)
}

pub(crate) fn save_atex_as_png_preserve_alpha(
    atex_bytes: &[u8],
    out_path: &Path,
) -> anyhow::Result<()> {
    save_atex_as_png_with_alpha_policy(atex_bytes, out_path, AlphaPolicy::Preserve)
}

pub(crate) fn save_rgba_as_png(
    width: u32,
    height: u32,
    rgba: &[u8],
    out_path: &Path,
) -> anyhow::Result<()> {
    let expected_len = checked_rgba_len(width as usize, height as usize, "RGBA")?;
    if rgba.len() != expected_len {
        bail!(
            "RGBA buffer length {} does not match dimensions {}x{}",
            rgba.len(),
            width,
            height
        );
    }
    save_buffer_with_format(
        out_path,
        rgba,
        width,
        height,
        ColorType::Rgba8,
        ImageFormat::Png,
    )
    .with_context(|| format!("writing PNG {}", out_path.display()))?;
    Ok(())
}

pub(crate) fn decode_dds_rgba(dds_bytes: &[u8]) -> anyhow::Result<(u32, u32, Vec<u8>, String)> {
    let magic = dds_bytes
        .get(..4)
        .context("DDS payload shorter than magic")?;
    if magic != b"DDS " {
        bail!("not a DDS texture");
    }
    let header_size = read_u32(dds_bytes, 4).context("DDS missing header size")?;
    if header_size != 124 {
        bail!("unsupported DDS header size {header_size}");
    }
    let height = read_u32(dds_bytes, 12).context("DDS missing height")?;
    let width = read_u32(dds_bytes, 16).context("DDS missing width")?;
    if width == 0 || height == 0 {
        bail!("unsupported DDS dimensions {width}x{height}");
    }
    let pf_flags = read_u32(dds_bytes, 80).context("DDS missing pixel-format flags")?;
    let fourcc = dds_bytes
        .get(84..88)
        .context("DDS missing pixel-format FourCC")?;
    let rgb_bit_count = read_u32(dds_bytes, 88).context("DDS missing RGB bit count")?;
    let masks = [
        read_u32(dds_bytes, 92).context("DDS missing R mask")?,
        read_u32(dds_bytes, 96).context("DDS missing G mask")?,
        read_u32(dds_bytes, 100).context("DDS missing B mask")?,
        read_u32(dds_bytes, 104).context("DDS missing A mask")?,
    ];
    let payload = dds_bytes
        .get(128..)
        .context("DDS missing texture payload")?;
    let (rgba, format) = match fourcc {
        b"DXT1" => (
            decode_dxt1_rgba(payload, width as usize, height as usize)?,
            "DXT1".to_string(),
        ),
        b"DXT3" => (
            decode_dxt3_rgba(payload, width as usize, height as usize)?,
            "DXT3".to_string(),
        ),
        b"DXT5" => (
            decode_dxt5_rgba(payload, width as usize, height as usize, false)?,
            "DXT5".to_string(),
        ),
        b"\0\0\0\0" if pf_flags & 0x40 != 0 => (
            decode_uncompressed_dds_rgba(payload, width, height, rgb_bit_count, masks)?,
            format!("RGB{rgb_bit_count}"),
        ),
        other => bail!(
            "unsupported DDS FourCC {} flags=0x{pf_flags:x} rgb_bits={rgb_bit_count}",
            String::from_utf8_lossy(other)
        ),
    };
    Ok((width, height, rgba, format))
}

fn decode_uncompressed_dds_rgba(
    payload: &[u8],
    width: u32,
    height: u32,
    rgb_bit_count: u32,
    masks: [u32; 4],
) -> anyhow::Result<Vec<u8>> {
    let bytes_per_pixel = (rgb_bit_count / 8) as usize;
    if !matches!(bytes_per_pixel, 2..=4) {
        bail!("unsupported uncompressed DDS bit depth {rgb_bit_count}");
    }
    let width = usize::try_from(width).context("DDS width does not fit usize")?;
    let height = usize::try_from(height).context("DDS height does not fit usize")?;
    let pixel_count = checked_pixel_count(width, height, "uncompressed DDS")?;
    let expected = pixel_count
        .checked_mul(bytes_per_pixel)
        .context("DDS dimensions overflow")?;
    if payload.len() < expected {
        bail!(
            "uncompressed DDS payload underrun: have {}, need {expected}",
            payload.len()
        );
    }

    let mut rgba = vec![0_u8; checked_rgba_len(width, height, "uncompressed DDS")?];
    for i in 0..pixel_count {
        let src = i * bytes_per_pixel;
        let raw = match bytes_per_pixel {
            2 => u32::from(u16::from_le_bytes([payload[src], payload[src + 1]])),
            3 => {
                u32::from(payload[src])
                    | (u32::from(payload[src + 1]) << 8)
                    | (u32::from(payload[src + 2]) << 16)
            }
            4 => u32::from_le_bytes([
                payload[src],
                payload[src + 1],
                payload[src + 2],
                payload[src + 3],
            ]),
            _ => unreachable!(),
        };
        let dst = i * 4;
        rgba[dst] = extract_masked_channel(raw, masks[0]);
        rgba[dst + 1] = extract_masked_channel(raw, masks[1]);
        rgba[dst + 2] = extract_masked_channel(raw, masks[2]);
        rgba[dst + 3] = if masks[3] == 0 {
            255
        } else {
            extract_masked_channel(raw, masks[3])
        };
    }
    Ok(rgba)
}

fn extract_masked_channel(raw: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max = mask >> shift;
    let value = (raw & mask) >> shift;
    ((value * 255 + max / 2) / max) as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlphaPolicy {
    Preserve,
    ForceOpaque,
}

fn save_atex_as_png_with_alpha_policy(
    atex_bytes: &[u8],
    out_path: &Path,
    alpha_policy: AlphaPolicy,
) -> anyhow::Result<()> {
    let (width, height, mut rgba) = decode_atex_rgba(atex_bytes)?;
    if alpha_policy == AlphaPolicy::ForceOpaque {
        // Skill icons use client-specific alpha/luminance data; ordinary PNG viewers
        // display that as false transparency, so skill export keeps the existing
        // opaque policy. Inventory-icon investigations can preserve alpha separately.
        for chunk in rgba.chunks_exact_mut(4) {
            chunk[3] = 255;
        }
    }
    let image = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, rgba)
        .context("decoded ATEX RGBA buffer has invalid dimensions")?;
    image
        .save(out_path)
        .with_context(|| format!("writing PNG {}", out_path.display()))?;
    Ok(())
}

pub(crate) fn decode_atex_rgba(atex_bytes: &[u8]) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    let header = parse_header(atex_bytes)?;
    let dxt = decompress_atex_to_dxt5(atex_bytes, header)?;
    let rgba = match header.format {
        AtexDxtFormat::Dxt1 => {
            decode_dxt1_rgba(&dxt, header.width as usize, header.height as usize)?
        }
        AtexDxtFormat::Dxt2 | AtexDxtFormat::Dxt3 | AtexDxtFormat::DxtN => {
            decode_dxt3_rgba(&dxt, header.width as usize, header.height as usize)?
        }
        AtexDxtFormat::Dxt4 | AtexDxtFormat::Dxt5 | AtexDxtFormat::DxtA | AtexDxtFormat::DxtL => {
            let premultiply = header.format == AtexDxtFormat::DxtL;
            decode_dxt5_rgba(
                &dxt,
                header.width as usize,
                header.height as usize,
                premultiply,
            )?
        }
    };
    Ok((header.width as u32, header.height as u32, rgba))
}

fn checked_pixel_count(width: usize, height: usize, context: &str) -> anyhow::Result<usize> {
    let pixels = width
        .checked_mul(height)
        .with_context(|| format!("{context} pixel count overflow"))?;
    if pixels > MAX_TEXTURE_PIXELS {
        bail!("{context} texture too large: {width}x{height}");
    }
    Ok(pixels)
}

fn checked_rgba_len(width: usize, height: usize, context: &str) -> anyhow::Result<usize> {
    checked_pixel_count(width, height, context)?
        .checked_mul(4)
        .with_context(|| format!("{context} RGBA byte count overflow"))
}

fn dxt_block_count(width: usize, height: usize, context: &str) -> anyhow::Result<usize> {
    checked_pixel_count(width, height, context)?;
    let blocks_x = width.div_ceil(4);
    let blocks_y = height.div_ceil(4);
    blocks_x
        .checked_mul(blocks_y)
        .with_context(|| format!("{context} DXT block count overflow"))
}

fn decompress_atex_to_dxt5(atex_bytes: &[u8], header: AtexHeader) -> anyhow::Result<Vec<u8>> {
    if !atex_bytes.len().is_multiple_of(4) {
        bail!("ATEX payload length is not 32-bit aligned");
    }
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
    let block_bytes = if header.format == AtexDxtFormat::Dxt1 {
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
    let alpha_data_size2 = 0;
    let alpha_data_size = usize::from((format_flags & 640) != 0) * 2;
    let color_data_size = usize::from((format_flags & 528) != 0) * 2;
    let block_size = alpha_data_size2 + alpha_data_size + color_data_size;
    if block_size != 2 && block_size != 4 {
        bail!("unsupported ATEX block layout size {block_size}");
    }

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

fn decode_dxt5_rgba(
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
fn decode_dxt3_rgba(dxt: &[u8], width: usize, height: usize) -> anyhow::Result<Vec<u8>> {
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

fn decode_dxt1_rgba(dxt: &[u8], width: usize, height: usize) -> anyhow::Result<Vec<u8>> {
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
        let alpha_index = ((alpha_bits >> (3 * i)) & 7) as usize;
        let color_index = ((color_bits >> (2 * i)) & 3) as usize;
        let (r, g, b) = color_table[color_index];
        *px = (r, g, b, alpha_table[alpha_index]);
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
    fn uncompressed_dds_rejects_oversized_dimensions_before_payload_len() {
        let mut dds = vec![0_u8; 128];
        dds[..4].copy_from_slice(b"DDS ");
        dds[4..8].copy_from_slice(&124_u32.to_le_bytes());
        dds[12..16].copy_from_slice(&8193_u32.to_le_bytes());
        dds[16..20].copy_from_slice(&8193_u32.to_le_bytes());
        dds[80..84].copy_from_slice(&0x40_u32.to_le_bytes());
        dds[88..92].copy_from_slice(&32_u32.to_le_bytes());

        let err = decode_dds_rgba(&dds).expect_err("oversized DDS must fail before allocation");

        assert!(err.to_string().contains("texture too large"));
    }
}
