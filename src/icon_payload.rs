use std::path::Path;

use crate::atex::{self, AtexContainer, AtexHeader};

pub(crate) enum IconPayload<'a> {
    Atex {
        bytes: &'a [u8],
        header: AtexHeader,
    },
    FfnaInlineAtex {
        offset: usize,
        bytes: &'a [u8],
        header: AtexHeader,
    },
    Dds {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        format: String,
    },
}

impl IconPayload<'_> {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Atex { header, .. } => match header.container {
                AtexContainer::Atex => "atex",
                AtexContainer::Attx => "attx",
            },
            Self::FfnaInlineAtex { header, .. } => match header.container {
                AtexContainer::Atex => "ffna_inline_atex",
                AtexContainer::Attx => "ffna_inline_attx",
            },
            Self::Dds { .. } => "dds",
        }
    }

    pub(crate) fn width(&self) -> u32 {
        match self {
            Self::Atex { header, .. } | Self::FfnaInlineAtex { header, .. } => {
                u32::from(header.width)
            }
            Self::Dds { width, .. } => *width,
        }
    }

    pub(crate) fn height(&self) -> u32 {
        match self {
            Self::Atex { header, .. } | Self::FfnaInlineAtex { header, .. } => {
                u32::from(header.height)
            }
            Self::Dds { height, .. } => *height,
        }
    }

    pub(crate) fn format(&self) -> &str {
        match self {
            Self::Atex { header, .. } | Self::FfnaInlineAtex { header, .. } => {
                header.format.as_fourcc()
            }
            Self::Dds { format, .. } => format,
        }
    }

    pub(crate) fn inline_texture_offset(&self) -> Option<usize> {
        match self {
            Self::FfnaInlineAtex { offset, .. } => Some(*offset),
            _ => None,
        }
    }

    pub(crate) fn save_png(&self, path: &Path) -> anyhow::Result<()> {
        match self {
            Self::Atex { bytes, .. } | Self::FfnaInlineAtex { bytes, .. } => {
                atex::save_atex_as_png_preserve_alpha(bytes, path)
            }
            Self::Dds {
                width,
                height,
                rgba,
                ..
            } => atex::save_rgba_as_png(*width, *height, rgba, path),
        }
    }
}

pub(crate) fn decode_icon_payload(bytes: &[u8]) -> anyhow::Result<Option<IconPayload<'_>>> {
    if bytes.starts_with(b"ATEX") || bytes.starts_with(b"ATTX") {
        let header = atex::parse_header(bytes)?;
        return Ok(Some(IconPayload::Atex { bytes, header }));
    }

    if bytes.get(..4) == Some(b"ffna") {
        let Some((offset, bytes, header)) = find_inline_atex_payload(bytes) else {
            return Ok(None);
        };
        return Ok(Some(IconPayload::FfnaInlineAtex {
            offset,
            bytes,
            header,
        }));
    }

    if bytes.get(..4) == Some(b"DDS ") {
        let (width, height, rgba, format) = atex::decode_dds_rgba(bytes)?;
        return Ok(Some(IconPayload::Dds {
            width,
            height,
            rgba,
            format,
        }));
    }

    Ok(None)
}

pub(crate) fn find_inline_atex_payload(bytes: &[u8]) -> Option<(usize, &[u8], AtexHeader)> {
    find_inline_atex_payloads(bytes).next()
}

pub(crate) fn find_inline_atex_payloads(
    bytes: &[u8],
) -> impl Iterator<Item = (usize, &[u8], AtexHeader)> {
    let max_start = bytes.len().saturating_sub(20);
    (0..=max_start).filter_map(move |offset| {
        let magic = bytes.get(offset..offset + 4)?;
        if magic != b"ATEX" && magic != b"ATTX" {
            return None;
        }
        let payload_len = bytes.len() - offset;
        let aligned_len = payload_len - (payload_len % 4);
        if aligned_len < 20 {
            return None;
        }
        let payload = &bytes[offset..offset + aligned_len];
        let header = atex::parse_header(payload).ok()?;
        Some((offset, payload, header))
    })
}
