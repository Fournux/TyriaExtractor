use anyhow::{Context, Result, bail};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use super::{PacketRow, ServiceNpcKey};

pub(crate) fn captured_npc_name_words(path: &Path) -> Result<BTreeSet<Vec<u16>>> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut names = BTreeSet::new();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line =
            line.with_context(|| format!("reading {} line {}", path.display(), line_index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let row: PacketRow = serde_json::from_str(&line)
            .with_context(|| format!("parsing {} line {}", path.display(), line_index + 1))?;
        if row.kind == "world_packet" && row.header == 0x009b {
            let words = fixed_words(&packet_bytes(&row)?, 8, 32);
            if words.is_empty() {
                bail!(
                    "{} line {} AGENT_UPDATE_NPC_NAME contains an empty name",
                    path.display(),
                    line_index + 1
                );
            }
            names.insert(words);
        }
    }
    Ok(names)
}

pub(super) fn observe_npc_packet(
    row: &PacketRow,
    current_maps: &mut BTreeMap<u64, u32>,
    agent_npcs: &mut BTreeMap<(u64, u32), ServiceNpcKey>,
    agent_names: &mut BTreeMap<(u64, u32), Vec<u16>>,
    service_name_words: &mut BTreeMap<ServiceNpcKey, Vec<u16>>,
) -> Result<()> {
    let bytes = packet_bytes(row)?;
    match row.header {
        0x0199 => {
            let map_id = required_u32(&bytes, 8, "INSTANCE_LOAD_INFO map_id")?;
            agent_npcs.retain(|(session_id, _), _| *session_id != row.session_id);
            agent_names.retain(|(session_id, _), _| *session_id != row.session_id);
            if map_id == 0 {
                current_maps.remove(&row.session_id);
            } else {
                current_maps.insert(row.session_id, map_id);
            }
        }
        0x0020 => {
            let agent_id = required_u32(&bytes, 4, "AGENT_SPAWNED agent_id")?;
            let composite = required_u32(&bytes, 8, "AGENT_SPAWNED agent type")?;
            agent_npcs.remove(&(row.session_id, agent_id));
            if composite & 0xf000_0000 == 0x2000_0000 {
                let Some(map_id) = current_maps.get(&row.session_id).copied() else {
                    return Ok(());
                };
                let position_x_bits = required_u32(&bytes, 0x14, "AGENT_SPAWNED position.x")?;
                let position_y_bits = required_u32(&bytes, 0x18, "AGENT_SPAWNED position.y")?;
                if !f32::from_bits(position_x_bits).is_finite()
                    || !f32::from_bits(position_y_bits).is_finite()
                {
                    bail!("AGENT_SPAWNED position is not finite");
                }
                let agent_key = (row.session_id, agent_id);
                let service_npc = ServiceNpcKey {
                    map_id,
                    npc_model_id: composite & 0x0fff_ffff,
                    position_x_bits,
                    position_y_bits,
                };
                if let Some(words) = agent_names.get(&agent_key) {
                    remember_service_name(service_name_words, service_npc, words)?;
                }
                agent_npcs.insert(agent_key, service_npc);
            }
        }
        0x009b => {
            let agent_id = required_u32(&bytes, 4, "AGENT_UPDATE_NPC_NAME agent_id")?;
            let words = fixed_words(&bytes, 8, 32);
            if words.is_empty() {
                bail!("AGENT_UPDATE_NPC_NAME name is empty");
            }
            let agent_key = (row.session_id, agent_id);
            if agent_names
                .insert(agent_key, words.clone())
                .is_some_and(|previous| previous != words)
            {
                bail!("agent {agent_id} changed its encoded name");
            }
            if let Some(service_npc) = agent_npcs.get(&agent_key).copied() {
                remember_service_name(service_name_words, service_npc, &words)?;
            }
        }
        0x0021 => {
            let agent_id = required_u32(&bytes, 4, "AGENT_DESPAWNED agent_id")?;
            agent_npcs.remove(&(row.session_id, agent_id));
            agent_names.remove(&(row.session_id, agent_id));
        }
        _ => {}
    }
    Ok(())
}

fn remember_service_name(
    service_name_words: &mut BTreeMap<ServiceNpcKey, Vec<u16>>,
    service_npc: ServiceNpcKey,
    words: &[u16],
) -> Result<()> {
    if service_name_words
        .insert(service_npc, words.to_vec())
        .is_some_and(|previous| previous != words)
    {
        bail!("service NPC {:?} changed its encoded name", service_npc);
    }
    Ok(())
}

fn packet_bytes(row: &PacketRow) -> Result<Vec<u8>> {
    let bytes = hex::decode(&row.raw_hex).context("packet raw_hex is invalid")?;
    if u32_at(&bytes, 0) != Some(row.header) {
        bail!("packet 0x{:04X} has a mismatched header", row.header);
    }
    Ok(bytes)
}

fn required_u32(bytes: &[u8], offset: usize, field: &str) -> Result<u32> {
    u32_at(bytes, offset).with_context(|| format!("{field} is truncated"))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes(raw.try_into().ok()?))
}

fn fixed_words(bytes: &[u8], offset: usize, capacity: usize) -> Vec<u16> {
    bytes
        .get(offset..offset.saturating_add(capacity.saturating_mul(2)))
        .unwrap_or_default()
        .chunks_exact(2)
        .map(|raw| u16::from_le_bytes([raw[0], raw[1]]))
        .take_while(|word| *word != 0)
        .collect()
}
