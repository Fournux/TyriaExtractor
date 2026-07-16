use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::text::hex_to_bytes;

const MAX_PLAUSIBLE_QUEST_ID: u32 = u16::MAX as u32;

#[derive(Debug)]
pub(super) struct QuestSnapshotRow {
    pub(super) quest_id: u32,
    pub(super) map_from: Option<u32>,
    pub(super) location_encoded: Option<Vec<u16>>,
    pub(super) name_encoded: Option<Vec<u16>>,
    pub(super) npc_encoded: Option<Vec<u16>>,
    pub(super) description_encoded: Option<Vec<u16>>,
    pub(super) objectives_encoded: Option<Vec<u16>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorldPacketRow {
    #[serde(default)]
    pub(super) ts_ms: u128,
    #[serde(default)]
    pub(super) session_id: u64,
    pub(super) header: u32,
    pub(super) raw_hex: String,
}

pub(super) fn parse_quest_packet(packet: &WorldPacketRow) -> Result<Option<QuestSnapshotRow>> {
    let row = match packet.header {
        0x49 => parse_quest_add_packet(packet).map(Some),
        0x4c => parse_quest_description_packet(packet).map(Some),
        0x50 => parse_quest_general_info_packet(packet).map(Some),
        0x54 => parse_quest_update_objectives_packet(packet).map(Some),
        _ => Ok(None),
    }?;
    Ok(row.filter(|row| (1..=MAX_PLAUSIBLE_QUEST_ID).contains(&row.quest_id)))
}

pub(super) fn parse_agent_spawned_packet(packet: &WorldPacketRow) -> Result<(u32, Option<u32>)> {
    let bytes = world_packet_bytes(packet, 0x20, 0x74)?;
    let agent_id = u32_at(&bytes, 4).context("AGENT_SPAWNED agent_id is truncated")?;
    let agent_type = u32_at(&bytes, 8).context("AGENT_SPAWNED agent_type is truncated")?;
    let npc_model_id =
        (agent_type & 0xf000_0000 == 0x2000_0000).then_some(agent_type & 0x0fff_ffff);
    Ok((agent_id, npc_model_id))
}
pub(super) fn parse_agent_despawned_packet(packet: &WorldPacketRow) -> Result<u32> {
    let bytes = world_packet_bytes(packet, 0x21, 8)?;
    u32_at(&bytes, 4).context("AGENT_DESPAWNED agent_id is truncated")
}

pub(super) fn parse_instance_load_info_packet(packet: &WorldPacketRow) -> Result<u32> {
    let bytes = world_packet_bytes(packet, 0x199, 0x1c)?;
    u32_at(&bytes, 8).context("INSTANCE_LOAD_INFO map_id is truncated")
}

pub(super) fn parse_npc_update_properties_packet(
    packet: &WorldPacketRow,
) -> Result<(u32, Option<Vec<u16>>)> {
    let bytes = world_packet_bytes(packet, 0x56, 0x34)?;
    let npc_model_id = u32_at(&bytes, 4).context("NPC_UPDATE_PROPERTIES npc_id is truncated")?;
    Ok((npc_model_id, fixed_utf16_words(&bytes, 36, 8)))
}

pub(super) fn parse_dialog_sender_packet(packet: &WorldPacketRow) -> Result<u32> {
    let bytes = world_packet_bytes(packet, 0x81, 8)?;
    u32_at(&bytes, 4).context("DIALOG_SENDER agent_id is truncated")
}

pub(super) fn parse_dialog_button_packet(
    packet: &WorldPacketRow,
) -> Result<Option<(u32, &'static str)>> {
    let bytes = world_packet_bytes(packet, 0x7e, 0x110)?;
    let dialog_id = u32_at(&bytes, 264).context("DIALOG_BUTTON dialog_id is truncated")?;
    if dialog_id & 0x0080_0000 == 0 {
        return Ok(None);
    }
    let quest_id = (dialog_id ^ 0x0080_0000) >> 8;
    if !(1..=MAX_PLAUSIBLE_QUEST_ID).contains(&quest_id) {
        return Ok(None);
    }
    let dialog_type = match dialog_id & 0x0000_000f {
        1 => "take",
        2 => "decline",
        3 => "enquire",
        4 => "enquire_next",
        5 => "recap",
        6 => "enquire_reward",
        7 => "reward",
        _ => "unknown",
    };
    Ok(Some((quest_id, dialog_type)))
}
pub(super) fn parse_quest_remove_id(packet: &WorldPacketRow) -> Result<u32> {
    let bytes = world_packet_bytes(packet, 0x52, 8)?;
    quest_id(&bytes)
}

pub(super) fn parse_quest_add_packet(packet: &WorldPacketRow) -> Result<QuestSnapshotRow> {
    let bytes = world_packet_bytes(packet, 0x49, 0x50)?;
    let mut row = partial_quest_row(quest_id(&bytes)?);
    row.map_from = u32_at(&bytes, 76);
    row.location_encoded = fixed_utf16_words(&bytes, 28, 8);
    row.name_encoded = fixed_utf16_words(&bytes, 44, 8);
    row.npc_encoded = fixed_utf16_words(&bytes, 60, 8);
    Ok(row)
}

pub(super) fn parse_quest_description_packet(packet: &WorldPacketRow) -> Result<QuestSnapshotRow> {
    let bytes = world_packet_bytes(packet, 0x4c, 0x208)?;
    let mut row = partial_quest_row(quest_id(&bytes)?);
    row.description_encoded = fixed_utf16_words(&bytes, 8, 128);
    row.objectives_encoded = fixed_utf16_words(&bytes, 264, 128);
    Ok(row)
}

pub(super) fn parse_quest_general_info_packet(packet: &WorldPacketRow) -> Result<QuestSnapshotRow> {
    let bytes = world_packet_bytes(packet, 0x50, 0x40)?;
    let mut row = partial_quest_row(quest_id(&bytes)?);
    row.location_encoded = fixed_utf16_words(&bytes, 12, 8);
    row.name_encoded = fixed_utf16_words(&bytes, 28, 8);
    row.npc_encoded = fixed_utf16_words(&bytes, 44, 8);
    row.map_from = u32_at(&bytes, 60);
    Ok(row)
}

pub(super) fn parse_quest_update_objectives_packet(
    packet: &WorldPacketRow,
) -> Result<QuestSnapshotRow> {
    let bytes = world_packet_bytes(packet, 0x54, 0x108)?;
    let mut row = partial_quest_row(quest_id(&bytes)?);
    row.objectives_encoded = fixed_utf16_words(&bytes, 8, 128);
    Ok(row)
}

pub(super) fn world_packet_bytes(
    packet: &WorldPacketRow,
    expected_header: u32,
    expected_size: usize,
) -> Result<Vec<u8>> {
    if packet.header != expected_header {
        bail!(
            "packet row header 0x{:04X} does not match 0x{expected_header:04X}",
            packet.header
        );
    }
    let bytes = hex_to_bytes(&packet.raw_hex).context("quest packet raw_hex is not valid hex")?;
    if bytes.len() != expected_size {
        bail!(
            "quest packet 0x{expected_header:04X} has {} bytes instead of {expected_size}",
            bytes.len()
        );
    }
    if u32_at(&bytes, 0) != Some(expected_header) {
        bail!("raw quest packet header is not 0x{expected_header:04X}");
    }
    Ok(bytes)
}

pub(super) fn quest_id(bytes: &[u8]) -> Result<u32> {
    u32_at(bytes, 4).context("quest packet quest_id is truncated")
}

pub(super) fn partial_quest_row(quest_id: u32) -> QuestSnapshotRow {
    QuestSnapshotRow {
        quest_id,
        map_from: None,
        location_encoded: None,
        name_encoded: None,
        npc_encoded: None,
        description_encoded: None,
        objectives_encoded: None,
    }
}

pub(super) fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

pub(super) fn fixed_utf16_words(bytes: &[u8], offset: usize, capacity: usize) -> Option<Vec<u16>> {
    let mut words = Vec::with_capacity(capacity);
    for index in 0..capacity {
        let start = offset.checked_add(index.checked_mul(2)?)?;
        let word = u16::from_le_bytes(bytes.get(start..start.checked_add(2)?)?.try_into().ok()?);
        if word == 0 {
            break;
        }
        words.push(word);
    }
    (!words.is_empty()).then_some(words)
}
