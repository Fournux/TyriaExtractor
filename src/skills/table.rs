use anyhow::Context;

use crate::pe::PeImage;

pub(super) const SKILL_RECORD_SIZE: usize = 164;

pub(super) struct SkillTableDetection {
    pub(super) file_offset: usize,
    pub(super) record_count: usize,
    score: usize,
}

fn read_u32_at(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn skill_table_probe_score(data: &[u8], offset: usize, record_count: usize) -> Option<usize> {
    if offset + record_count.checked_mul(SKILL_RECORD_SIZE)? > data.len() {
        return None;
    }
    if read_u32_at(data, offset)? != 0 {
        return None;
    }
    let declared_count = read_u32_at(data, offset + 0x2c)? as usize;
    if declared_count != record_count {
        return None;
    }

    let first_live = offset + SKILL_RECORD_SIZE;
    if read_u32_at(data, first_live)? != 1 {
        return None;
    }
    let first_name = read_u32_at(data, first_live + 0x98)?;
    let first_desc = read_u32_at(data, first_live + 0xa0)?;
    if first_name == 0 || first_desc != first_name + 1 {
        return None;
    }

    let probe_count = record_count.min(256);
    let mut score = 0_usize;
    let mut live_like = 0_usize;
    for index in 1..probe_count {
        let rec = offset + index * SKILL_RECORD_SIZE;
        let name_id = read_u32_at(data, rec + 0x98)?;
        let desc_id = read_u32_at(data, rec + 0xa0)?;
        if name_id == 0 {
            continue;
        }
        let campaign = read_u32_at(data, rec + 0x08)?;
        let type_code = read_u32_at(data, rec + 0x0c)?;
        let profession = *data.get(rec + 0x28)?;
        let equip_type = *data.get(rec + 0x33)?;

        if desc_id == name_id + 1 {
            score += 4;
            live_like += 1;
        }
        if campaign <= 4 {
            score += 1;
        }
        if type_code <= 29 {
            score += 1;
        }
        if profession <= 10 {
            score += 1;
        }
        if equip_type <= 3 {
            score += 1;
        }
    }

    if live_like < 32 {
        return None;
    }
    Some(score)
}

pub(super) fn locate_skill_table(
    pe_data: &[u8],
    pe: &PeImage,
) -> anyhow::Result<SkillTableDetection> {
    let mut best: Option<SkillTableDetection> = None;
    for section in pe.sections() {
        let raw_start = section.raw_pointer as usize;
        let raw_end = raw_start
            .saturating_add(section.raw_size as usize)
            .min(pe_data.len());
        if raw_end <= raw_start + SKILL_RECORD_SIZE * 2 {
            continue;
        }
        let mut offset = raw_start;
        while offset + SKILL_RECORD_SIZE * 2 <= raw_end {
            let Some(record_count) =
                read_u32_at(pe_data, offset + 0x2c).map(|value| value as usize)
            else {
                break;
            };
            let Some(table_end) = record_count
                .checked_mul(SKILL_RECORD_SIZE)
                .and_then(|len| offset.checked_add(len))
            else {
                offset += 4;
                continue;
            };
            if (512..=10000).contains(&record_count)
                && table_end <= raw_end
                && let Some(score) = skill_table_probe_score(pe_data, offset, record_count)
            {
                let candidate = SkillTableDetection {
                    file_offset: offset,
                    record_count,
                    score,
                };
                if best
                    .as_ref()
                    .is_none_or(|current| candidate.score > current.score)
                {
                    best = Some(candidate);
                }
            }
            offset += 4;
        }
    }

    best.with_context(|| "failed to locate s_skill table structurally in client PE")
}
