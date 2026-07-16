use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};

use super::*;

pub(super) fn drain_records() -> Vec<PacketRecord> {
    drain_queue(&RING, RING_CAP)
}
pub(super) fn drain_runtime_item_string_records() -> Vec<RuntimeItemStringsRecord> {
    drain_queue(&RUNTIME_ITEM_STRING_RING, RING_CAP)
}

pub(super) fn drain_world_packet_records() -> Vec<WorldPacketRecord> {
    drain_queue(&WORLD_PACKET_RING, WORLD_RING_CAP)
}

pub(super) fn drain_collector_offer_records() -> Vec<CollectorOfferRecord> {
    drain_queue(&COLLECTOR_OFFER_RING, COLLECTOR_RING_CAP)
}

pub(super) fn drain_vendor_catalog_records() -> Vec<VendorCatalogRecord> {
    drain_queue(&VENDOR_CATALOG_RING, VENDOR_RING_CAP)
}

pub(super) fn drain_decode_id_records() -> Vec<DecodeIdRecord> {
    drain_queue(&DECODE_ID_RING, RING_CAP)
}

pub(super) fn drain_text_resource_ref_records() -> Vec<TextResourceRefRecord> {
    drain_queue(&TEXT_RESOURCE_REF_RING, RING_CAP)
}

pub(super) fn drain_text_trace_records() -> Vec<TextDecodeTraceRecord> {
    drain_queue(&TEXT_TRACE_RING, RING_CAP)
}

fn append_rows_to<T>(
    path: &Path,
    records: &[T],
    mut write_row: impl FnMut(&mut BufWriter<File>, &T) -> io::Result<()>,
) -> io::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut out = BufWriter::with_capacity(64 * 1024, file);
    for record in records {
        write_row(&mut out, record)?;
    }
    out.flush()
}

fn append_rows<T>(
    records: &[T],
    write_row: impl FnMut(&mut BufWriter<File>, &T) -> io::Result<()>,
) -> io::Result<()> {
    let result = append_rows_to(output_path(), records, write_row);
    if result.is_err() {
        GENERAL_WRITE_FAILURES.fetch_add(1, Ordering::Relaxed);
    }
    result
}

fn append_world_rows<T>(
    records: &[T],
    write_row: impl FnMut(&mut BufWriter<File>, &T) -> io::Result<()>,
) -> io::Result<()> {
    append_world_rows_to(&quest_output_path(), records, write_row)
}

fn append_world_rows_to<T>(
    path: &Path,
    records: &[T],
    write_row: impl FnMut(&mut BufWriter<File>, &T) -> io::Result<()>,
) -> io::Result<()> {
    let result = append_rows_to(path, records, write_row);
    if result.is_err() {
        WORLD_WRITE_FAILURES.fetch_add(1, Ordering::Relaxed);
    }
    result
}

pub(super) fn append_world_packet_schema_records(
    records: &[WorldPacketSchemaRecord],
    session_id: u64,
    client_pe_timestamp: Option<u32>,
) -> io::Result<()> {
    append_world_rows_to(&capture_metadata_output_path(), records, |out, record| {
        write_world_packet_schema_record(out, record, session_id, client_pe_timestamp)
    })
}

fn write_world_packet_schema_record(
    mut out: impl Write,
    record: &WorldPacketSchemaRecord,
    session_id: u64,
    client_pe_timestamp: Option<u32>,
) -> io::Result<()> {
    write!(
        out,
        "{{\"kind\":\"world_packet_schema\",\"session_id\":{},\"header\":{},\"header_hex\":\"0x{:04X}\",\"name\":\"{}\",\"field_count\":{},\"fields\":[",
        session_id,
        record.header,
        record.header,
        world_packet_name(record.header),
        record.field_count
    )?;
    for (index, field) in record.fields[..record.field_count as usize]
        .iter()
        .enumerate()
    {
        if index > 0 {
            write!(out, ",")?;
        }
        write!(out, "{field}")?;
    }
    if let Some(timestamp) = client_pe_timestamp {
        write!(out, "],\"client_pe_timestamp\":{timestamp}")?;
        return writeln!(out, "}}");
    }
    writeln!(out, "]}}")
}

pub(super) fn append_world_packet_records(records: &[WorldPacketRecord]) -> io::Result<()> {
    append_world_rows(records, |out, record| {
        write_world_packet_record(out, record)
    })
}

pub(super) fn append_npc_world_packet_records(records: &[WorldPacketRecord]) -> io::Result<()> {
    append_world_rows_to(&npc_output_path(), records, |out, record| {
        write_world_packet_record(out, record)
    })
}

pub(super) fn append_vendor_context_packet_records(
    records: &[WorldPacketRecord],
) -> io::Result<()> {
    append_world_rows_to(&vendor_context_output_path(), records, |out, record| {
        write_world_packet_record(out, record)
    })
}

fn write_world_packet_record(mut out: impl Write, record: &WorldPacketRecord) -> io::Result<()> {
    write!(
        out,
        "{{\"ts_ms\":{},\"session_id\":{},\"capture_seq\":{},\"kind\":\"world_packet\",\"header\":{},\"header_hex\":\"0x{:04X}\",\"name\":\"{}\",\"len\":{},\"raw_hex\":\"",
        record.ts_ms,
        record.session_id,
        record.capture_seq,
        record.header,
        record.header,
        world_packet_name(record.header),
        record.len
    )?;
    packet::write_hex(&mut out, &record.data[..record.len as usize])?;
    writeln!(out, "\"}}")
}

pub(super) fn append_collector_offer_records(records: &[CollectorOfferRecord]) -> io::Result<()> {
    append_world_rows_to(&collector_output_path(), records, |out, record| {
        write_collector_offer_record(out, record)
    })
}

fn write_collector_offer_record(
    mut out: impl Write,
    record: &CollectorOfferRecord,
) -> io::Result<()> {
    let captured = record.captured_reward_count as usize;
    let capture_complete = record.merchant_agent_id != 0
        && record.npc_model_id != 0
        && record.transaction_service == 2
        && record.trophy_item_id != 0
        && record.required_item_model_id != 0
        && (1..=u32::from(u16::MAX)).contains(&record.required_item_quantity)
        && record.reward_count != 0
        && record.rewards_readable
        && !record.rewards_truncated
        && captured == record.reward_count as usize
        && record.reward_resolved[..captured]
            .iter()
            .all(|resolved| *resolved);
    write!(
        out,
        "{{\"ts_ms\":{},\"session_id\":{},\"capture_seq\":{},\"kind\":\"collector_offers\",\"merchant_agent_id\":{},\"npc_model_id\":",
        record.ts_ms, record.session_id, record.capture_seq, record.merchant_agent_id
    )?;
    if record.npc_model_id == 0 {
        write!(out, "null")?;
    } else {
        write!(out, "{}", record.npc_model_id)?;
    }
    write!(
        out,
        ",\"window_transaction_type\":{},\"transaction_service\":{},\"trophy_item_id\":{},\"required_item\":{{\"model_id\":{},\"quantity\":{}}},\"reward_count\":{},\"captured_reward_count\":{},\"rewards_readable\":{},\"rewards_truncated\":{},\"capture_complete\":{},\"rewards\":[",
        record.window_transaction_type,
        record.transaction_service,
        record.trophy_item_id,
        record.required_item_model_id,
        record.required_item_quantity,
        record.reward_count,
        record.captured_reward_count,
        record.rewards_readable,
        record.rewards_truncated,
        capture_complete,
    )?;
    for index in 0..captured {
        if index > 0 {
            write!(out, ",")?;
        }
        write!(
            out,
            "{{\"item_id\":{},\"model_id\":",
            record.reward_item_ids[index]
        )?;
        if record.reward_resolved[index] {
            let raw_file_id = record.reward_model_file_ids[index];
            write!(
                out,
                "{},\"model_file_id\":{},\"model_file_id_raw\":{},\"item_type\":{}",
                record.reward_model_ids[index],
                raw_file_id & 0x7FFF_FFFF,
                raw_file_id,
                record.reward_item_types[index],
            )?;
        } else {
            write!(
                out,
                "null,\"model_file_id\":null,\"model_file_id_raw\":null,\"item_type\":null"
            )?;
        }
        write!(out, "}}")?;
    }
    writeln!(out, "]}}")
}

fn append_vendor_catalog_records_to(
    path: &Path,
    records: &[VendorCatalogRecord],
) -> io::Result<()> {
    append_world_rows_to(path, records, |out, record| {
        write_vendor_catalog_record(out, record)
    })
}

pub(super) fn append_merchant_catalog_records(records: &[VendorCatalogRecord]) -> io::Result<()> {
    append_vendor_catalog_records_to(&merchant_output_path(), records)
}

pub(super) fn append_crafter_catalog_records(records: &[VendorCatalogRecord]) -> io::Result<()> {
    append_vendor_catalog_records_to(&crafter_output_path(), records)
}

pub(super) fn append_skill_trainer_catalog_records(
    records: &[VendorCatalogRecord],
) -> io::Result<()> {
    append_vendor_catalog_records_to(&skill_trainer_output_path(), records)
}

pub(super) fn append_vendor_context_catalog_records(
    records: &[VendorCatalogRecord],
) -> io::Result<()> {
    append_vendor_catalog_records_to(&vendor_context_output_path(), records)
}

fn write_vendor_catalog_record(
    mut out: impl Write,
    record: &VendorCatalogRecord,
) -> io::Result<()> {
    let captured = record.captured_entry_count as usize;
    let entries_complete = match record.transaction_service {
        1 | 3 => record.entries[..captured]
            .iter()
            .all(|entry| entry.resolved),
        10 => record.entries[..captured]
            .iter()
            .all(|entry| entry.source_id != 0),
        _ => false,
    };
    let capture_complete = record.merchant_agent_id != 0
        && record.npc_model_id != 0
        && record.entry_count != 0
        && record.entries_readable
        && !record.entries_truncated
        && captured == record.entry_count as usize
        && entries_complete;
    let kind = match record.transaction_service {
        1 => "merchant_items",
        3 => "crafter_products",
        10 => "skill_trainer_skills",
        _ => "vendor_catalog",
    };
    write!(
        out,
        "{{\"ts_ms\":{},\"session_id\":{},\"capture_seq\":{},\"kind\":\"{}\",\"merchant_agent_id\":{},\"npc_model_id\":",
        record.ts_ms, record.session_id, record.capture_seq, kind, record.merchant_agent_id
    )?;
    if record.npc_model_id == 0 {
        write!(out, "null")?;
    } else {
        write!(out, "{}", record.npc_model_id)?;
    }
    write!(
        out,
        ",\"window_transaction_type\":{},\"transaction_service\":{},\"entry_count\":{},\"captured_entry_count\":{},\"entries_readable\":{},\"entries_truncated\":{},\"capture_complete\":{},\"entries\":[",
        record.window_transaction_type,
        record.transaction_service,
        record.entry_count,
        record.captured_entry_count,
        record.entries_readable,
        record.entries_truncated,
        capture_complete,
    )?;
    for (index, entry) in record.entries[..captured].iter().enumerate() {
        if index > 0 {
            write!(out, ",")?;
        }
        if record.transaction_service == 10 {
            write!(
                out,
                "{{\"skill_id\":{},\"availability_flags_raw\":{}}}",
                entry.source_id, entry.aux
            )?;
            continue;
        }
        write!(out, "{{\"item_id\":{},\"model_id\":", entry.source_id)?;
        if entry.resolved {
            write!(
                out,
                "{},\"model_file_id\":{},\"model_file_id_raw\":{},\"item_type\":{},\"base_value\":{}",
                entry.model_id,
                entry.model_file_id & 0x7FFF_FFFF,
                entry.model_file_id,
                entry.item_type,
                entry.base_value,
            )?;
        } else {
            write!(
                out,
                "null,\"model_file_id\":null,\"model_file_id_raw\":null,\"item_type\":null,\"base_value\":null"
            )?;
        }
        write!(out, "}}")?;
    }
    writeln!(out, "]}}")
}

pub(super) fn write_world_status(status: &str, addr: Option<usize>) -> io::Result<()> {
    let result = write_status_to(&capture_metadata_output_path(), status, addr);
    if result.is_err() {
        WORLD_WRITE_FAILURES.fetch_add(1, Ordering::Relaxed);
    }
    result
}

pub(super) fn write_vendor_status(status: &str, addr: Option<usize>) -> io::Result<()> {
    write_world_status(status, addr)
}

fn world_packet_name(header: u32) -> &'static str {
    match header {
        0x0020 => "AGENT_SPAWNED",
        0x0021 => "AGENT_DESPAWNED",
        0x0049 => "QUEST_ADD",
        0x004C => "QUEST_DESCRIPTION",
        0x0050 => "QUEST_GENERAL_INFO",
        0x0051 => "QUEST_UPDATE_MARKER",
        0x0052 => "QUEST_REMOVE",
        0x0053 => "QUEST_ADD_MARKER",
        0x0054 => "QUEST_UPDATE_OBJECTIVES",
        0x0056 => "NPC_UPDATE_PROPERTIES",
        0x007E => "DIALOG_BUTTON",
        0x0081 => "DIALOG_SENDER",
        0x009B => "AGENT_UPDATE_NPC_NAME",
        0x00C3 => "WINDOW_MERCHANT",
        0x00C4 => "WINDOW_OWNER",
        0x0199 => "INSTANCE_LOAD_INFO",
        _ => "UNKNOWN",
    }
}

fn write_json_string(out: &mut impl Write, value: &str) -> io::Result<()> {
    write!(out, "\"")?;
    for ch in value.chars() {
        match ch {
            '"' => write!(out, "\\\"")?,
            '\\' => write!(out, "\\\\")?,
            '\n' => write!(out, "\\n")?,
            '\r' => write!(out, "\\r")?,
            '\t' => write!(out, "\\t")?,
            ch if ch <= '\u{1f}' => write!(out, "\\u{:04x}", ch as u32)?,
            ch => write!(out, "{ch}")?,
        }
    }
    write!(out, "\"")
}

pub(super) fn append_text_resource_ref_records(
    records: &[TextResourceRefRecord],
) -> io::Result<()> {
    append_rows(records, |out, record| {
        write_text_resource_ref_record(out, record)
    })
}

fn write_text_resource_ref_record(
    mut out: impl Write,
    record: &TextResourceRefRecord,
) -> io::Result<()> {
    write!(
        out,
        "{{\"ts_ms\":{},\"kind\":\"text_resource_ref\",\"language_id\":{},\"decoded_id\":{},\"text_file_index\":{},\"record_index\":{},\"file_desc_u32\":[",
        record.ts_ms,
        record.language_id,
        record.decoded_id,
        record.text_file_index,
        record.record_index
    )?;
    for (index, chunk) in record.file_desc.chunks_exact(4).enumerate() {
        if index > 0 {
            write!(out, ",")?;
        }
        write!(
            out,
            "{}",
            u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
        )?;
    }
    write!(out, "],\"file_desc_hex\":\"")?;
    packet::write_hex(&mut out, &record.file_desc)?;
    writeln!(out, "\"}}")
}

pub(super) fn append_text_trace_records(records: &[TextDecodeTraceRecord]) -> io::Result<()> {
    append_rows(records, |out, record| write_text_trace_record(out, record))
}

fn write_text_trace_record(mut out: impl Write, record: &TextDecodeTraceRecord) -> io::Result<()> {
    write!(
        out,
        "{{\"ts_ms\":{},\"kind\":\"text_decode_trace\",\"language_id\":{},\"context\":{},\"record_ptr\":{},\"record_size\":{},\"compression_or_flags\":{},\"record_type\":{},\"record_subtype\":{},",
        record.ts_ms,
        record.language_id,
        record.context,
        record.record_ptr,
        record.record_size,
        record.compression_or_flags,
        record.record_type,
        record.record_subtype
    )?;
    if record.has_ref {
        write!(
            out,
            "\"ref_language_id\":{},\"decoded_id\":{},\"text_file_index\":{},\"record_index\":{},\"ref_age_ms\":{},",
            record.ref_language_id,
            record.ref_decoded_id,
            record.ref_text_file_index,
            record.ref_record_index,
            record.ref_age_ms
        )?;
    } else {
        write!(
            out,
            "\"ref_language_id\":null,\"decoded_id\":null,\"text_file_index\":null,\"record_index\":null,\"ref_age_ms\":null,"
        )?;
    }
    write!(
        out,
        "\"record_truncated\":{},\"record_hex\":\"",
        record.record_truncated
    )?;
    packet::write_hex(
        &mut out,
        &record.record_bytes[..record.record_bytes_len as usize],
    )?;
    write!(
        out,
        "\",\"output_ptr\":{},\"output_len\":{},\"output_truncated\":{},\"output_u16_hex\":\"",
        record.output_ptr, record.output_len, record.output_truncated
    )?;
    packet::write_u16_hex(&mut out, &record.output[..record.output_len as usize])?;
    write!(out, "\",\"output_preview\":")?;
    let output_preview = String::from_utf16_lossy(&record.output[..record.output_len as usize]);
    write_json_string(&mut out, &output_preview)?;
    write!(
        out,
        ",\"substitute_start\":{},\"substitute_end\":{},\"substitute_len\":{},\"substitute_truncated\":{},\"substitute_u16_hex\":\"",
        record.substitute_start,
        record.substitute_end,
        record.substitute_len,
        record.substitute_truncated
    )?;
    packet::write_u16_hex(
        &mut out,
        &record.substitute[..record.substitute_len as usize],
    )?;
    writeln!(out, "\"}}")
}

pub(super) fn append_runtime_item_string_records(
    records: &[RuntimeItemStringsRecord],
) -> io::Result<()> {
    append_rows(records, |out, record| {
        write_runtime_item_string_record(out, record)
    })
}

fn write_runtime_item_string_record(
    mut out: impl Write,
    record: &RuntimeItemStringsRecord,
) -> io::Result<()> {
    write!(
        out,
        "{{\"ts_ms\":{},\"session_id\":{},\"capture_seq\":{},\"kind\":\"runtime_item_strings\",\"item_id\":{},\"model_id\":{},\"model_file_id\":{}",
        record.ts_ms,
        record.session_id,
        record.capture_seq,
        record.item_id,
        record.model_id,
        record.model_file_id
    )?;
    if record.desc_complete {
        write!(out, ",\"desc_enc_hex\":\"")?;
        packet::write_u16_hex(&mut out, &record.desc[..record.desc_len as usize])?;
        write!(out, "\"")?;
    }
    if record.complete_name_complete {
        write!(out, ",\"complete_name_enc_hex\":\"")?;
        packet::write_u16_hex(
            &mut out,
            &record.complete_name[..record.complete_name_len as usize],
        )?;
        write!(out, "\"")?;
    }
    writeln!(
        out,
        ",\"desc_complete\":{},\"desc_truncated\":{},\"complete_name_complete\":{},\"complete_name_truncated\":{}}}",
        record.desc_complete,
        record.desc_truncated,
        record.complete_name_complete,
        record.complete_name_truncated
    )
}

pub(super) fn decode_id_key(record: &DecodeIdRecord) -> Vec<u8> {
    let mut key = Vec::with_capacity(record.encoded_len as usize * 2);
    for word in &record.encoded[..record.encoded_len as usize] {
        key.extend_from_slice(&word.to_le_bytes());
    }
    key
}

pub(super) fn append_decode_id_records(records: &[DecodeIdRecord]) -> io::Result<()> {
    append_rows(records, |out, record| write_decode_id_record(out, record))
}

fn write_decode_id_record(mut out: impl Write, record: &DecodeIdRecord) -> io::Result<()> {
    write!(
        out,
        "{{\"ts_ms\":{},\"kind\":\"text_decode_ids\",\"language_id\":{},\"encoded_hex\":\"",
        record.ts_ms, record.language_id
    )?;
    packet::write_u16_hex(&mut out, &record.encoded[..record.encoded_len as usize])?;
    write!(out, "\"")?;
    write_merged_decode_ids(&mut out, record)?;
    writeln!(out, "}}")
}

pub(super) fn append_records(
    records: &[PacketRecord],
    decode_ids_by_encoded: &BTreeMap<Vec<u8>, DecodeIdRecord>,
) -> io::Result<()> {
    append_rows(records, |out, record| {
        write_record(out, record, decode_ids_by_encoded)
    })
}

fn write_record(
    mut out: impl Write,
    record: &PacketRecord,
    decode_ids_by_encoded: &BTreeMap<Vec<u8>, DecodeIdRecord>,
) -> io::Result<()> {
    let data = &record.data[..record.len as usize];
    let mut json = Vec::new();
    if !packet::write_decoded(&mut json, record.header, data)? {
        if record.from_handler {
            write_handler_probe(&mut out, record, data)?;
        }
        return Ok(());
    }
    if json.pop() != Some(b'}') {
        out.write_all(&json)?;
        return writeln!(out);
    }
    out.write_all(&json)?;
    write!(
        out,
        ",\"ts_ms\":{},\"session_id\":{}",
        record.ts_ms, record.session_id
    )?;
    if let Some(decode_ids) = matching_decode_ids(record, data, decode_ids_by_encoded) {
        write_merged_decode_ids(&mut out, decode_ids)?;
    }
    writeln!(out, "}}")
}

fn matching_decode_ids<'a>(
    record: &PacketRecord,
    data: &[u8],
    decode_ids_by_encoded: &'a BTreeMap<Vec<u8>, DecodeIdRecord>,
) -> Option<&'a DecodeIdRecord> {
    if record.header != GAME_SMSG_ITEM_GENERAL_INFO as u32
        && record.header != GAME_SMSG_ITEM_REUSE_ID as u32
    {
        return None;
    }
    decode_ids_by_encoded.get(packet::encoded_name_bytes(
        data,
        packet::ITEM_GENERAL_NAME_START,
    ))
}

fn write_merged_decode_ids(out: &mut impl Write, record: &DecodeIdRecord) -> io::Result<()> {
    write!(out, ",\"decoded_ids\":[")?;
    for (index, id) in record.ids[..record.id_count as usize].iter().enumerate() {
        if index > 0 {
            write!(out, ",")?;
        }
        write!(out, "{id}")?;
    }
    write!(out, "],\"text_refs\":[")?;
    for (index, id) in record.ids[..record.id_count as usize].iter().enumerate() {
        if index > 0 {
            write!(out, ",")?;
        }
        write!(
            out,
            "{{\"decoded_id\":{},\"text_file_index\":{},\"record_index\":{}}}",
            id,
            id / 1024,
            id % 1024
        )?;
    }
    write!(
        out,
        "],\"decode_language_id\":{},\"decode_truncated\":{}",
        record.language_id,
        record.id_count as usize == MAX_CAPTURED_DECODE_IDS
    )
}

fn write_handler_probe(mut out: impl Write, record: &PacketRecord, data: &[u8]) -> io::Result<()> {
    write!(
        out,
        "{{\"ts_ms\":{},\"kind\":\"handler_probe\",\"header\":\"0x{:04X}\",\"arg0\":\"0x{:08X}\",\"arg2\":\"0x{:08X}\",\"arg3\":\"0x{:08X}\",\"len\":{},\"raw_hex\":\"",
        record.ts_ms,
        record.header,
        record.dispatch_arg0,
        record.dispatch_arg2,
        record.dispatch_arg3,
        record.len
    )?;
    packet::write_hex(&mut out, &data[..data.len().min(64)])?;
    writeln!(out, "\"}}")
}

fn write_status_to(path: &Path, status: &str, addr: Option<usize>) -> io::Result<()> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut out = BufWriter::new(file);
    write!(
        out,
        "{{\"ts_ms\":{},\"session_id\":{},\"kind\":\"status\",\"status\":\"{}\"",
        now_ms(),
        capture_session_id(),
        status
    )?;
    if let Some(addr) = addr {
        write!(out, ",\"hook_addr\":\"0x{addr:08X}\"")?;
    }
    if let Some(timestamp) = unsafe { hooks::client_pe_timestamp() } {
        write!(out, ",\"client_pe_timestamp\":{timestamp}")?;
    }
    writeln!(out, "}}")?;
    out.flush()
}

pub(super) fn write_status(status: &str, hook_addr: Option<usize>) -> io::Result<()> {
    let result = write_status_to(&capture_metadata_output_path(), status, hook_addr);
    if result.is_err() {
        GENERAL_WRITE_FAILURES.fetch_add(1, Ordering::Relaxed);
    }
    result
}

pub(super) fn write_capture_health() -> io::Result<()> {
    let session_id = capture_session_id();
    let ts_ms = now_ms();
    let [general_lock, general_capacity, general_writes, world_lock, world_capacity, world_writes] =
        capture_health_counters();
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(capture_metadata_output_path())?;
    let mut out = BufWriter::new(file);
    writeln!(
        out,
        "{{\"ts_ms\":{ts_ms},\"session_id\":{session_id},\"kind\":\"capture_health\",\"capture_format_version\":{CAPTURE_FORMAT_VERSION},\"general_dropped_on_lock\":{general_lock},\"general_dropped_on_capacity\":{general_capacity},\"general_write_failures\":{general_writes},\"world_dropped_on_lock\":{world_lock},\"world_dropped_on_capacity\":{world_capacity},\"world_write_failures\":{world_writes}}}"
    )?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn capture_contract_uses_tyria_names() {
        assert_eq!(VERBOSE_JSONL_ENV, "TYRIA_VERBOSE_JSONL");
        assert_eq!(CAPTURE_FORMAT_VERSION, 5);
        assert_eq!(world_packet_name(0x20), "AGENT_SPAWNED");
        assert_eq!(world_packet_name(0x56), "NPC_UPDATE_PROPERTIES");
        assert_eq!(world_packet_name(0x7e), "DIALOG_BUTTON");
        assert_eq!(world_packet_name(0x81), "DIALOG_SENDER");
        assert_eq!(world_packet_name(0x9b), "AGENT_UPDATE_NPC_NAME");
        assert_eq!(output_filename(), "tyria_items.jsonl");
        assert_eq!(QUESTS_JSONL_FILENAME, "tyria_quests.jsonl");
        assert_eq!(NPCS_JSONL_FILENAME, "tyria_npcs.jsonl");
        assert_eq!(VENDOR_CONTEXT_JSONL_FILENAME, "tyria_vendor_context.jsonl");
        assert_eq!(COLLECTORS_JSONL_FILENAME, "tyria_collectors.jsonl");
        assert_eq!(MERCHANTS_JSONL_FILENAME, "tyria_merchants.jsonl");
        assert_eq!(CRAFTERS_JSONL_FILENAME, "tyria_crafters.jsonl");
        assert_eq!(SKILL_TRAINERS_JSONL_FILENAME, "tyria_skill_trainers.jsonl");
        assert_eq!(CAPTURE_METADATA_JSONL_FILENAME, "tyria_capture.jsonl");
    }

    #[test]
    fn world_records_preserve_client_schema_and_packet_bytes() {
        let mut schema = WorldPacketSchemaRecord {
            header: 0x49,
            field_count: 3,
            fields: [0; MAX_WORLD_SCHEMA_FIELDS],
        };
        schema.fields[..3].copy_from_slice(&[4, 1028, 2055]);
        let mut schema_out = Vec::new();
        write_world_packet_schema_record(&mut schema_out, &schema, 7, Some(123)).unwrap();
        let schema_row: Value = serde_json::from_slice(&schema_out).unwrap();
        assert_eq!(schema_row["kind"], "world_packet_schema");
        assert_eq!(schema_row["name"], "QUEST_ADD");
        assert_eq!(schema_row["session_id"], 7);
        assert_eq!(schema_row["client_pe_timestamp"], 123);
        assert_eq!(schema_row["fields"], serde_json::json!([4, 1028, 2055]));

        let mut packet = WorldPacketRecord {
            ts_ms: 11,
            session_id: 7,
            header: 0x49,
            capture_seq: 3,
            len: 8,
            data: [0; MAX_WORLD_PACKET_BYTES],
        };
        packet.data[..8].copy_from_slice(&[0x49, 0, 0, 0, 0x89, 3, 0, 0]);
        let mut packet_out = Vec::new();
        write_world_packet_record(&mut packet_out, &packet).unwrap();
        let packet_row: Value = serde_json::from_slice(&packet_out).unwrap();
        assert_eq!(packet_row["kind"], "world_packet");
        assert_eq!(packet_row["session_id"], 7);
        assert_eq!(packet_row["capture_seq"], 3);
        assert_eq!(packet_row["header"], 0x49);
        assert_eq!(packet_row["raw_hex"], "4900000089030000");
    }

    #[test]
    fn runtime_item_strings_preserve_complete_encstrings() {
        let mut record = RuntimeItemStringsRecord::new(101);
        record.ts_ms = 1;
        record.session_id = 7;
        record.capture_seq = 3;
        record.model_id = 9090;
        record.model_file_id = 202;
        record.desc_len = 3;
        record.desc_complete = true;
        record.desc[..3].copy_from_slice(&[0x022c, 0x0290, 0]);
        record.complete_name_len = 3;
        record.complete_name_complete = true;
        record.complete_name[..3].copy_from_slice(&[0x022c, 0x0164, 0]);
        let mut out = Vec::new();

        write_runtime_item_string_record(&mut out, &record).unwrap();

        let row: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(row["kind"], "runtime_item_strings");
        assert_eq!(row["item_id"], 101);
        assert_eq!(row["model_id"], 9090);
        assert_eq!(row["model_file_id"], 202);
        assert_eq!(row["desc_enc_hex"], "2c0290020000");
        assert_eq!(row["complete_name_enc_hex"], "2c0264010000");
        assert_eq!(row["desc_complete"], true);
        assert_eq!(row["complete_name_complete"], true);
    }

    #[test]
    fn collector_offer_record_preserves_stable_exchange_terms() {
        let mut record = CollectorOfferRecord {
            ts_ms: 11,
            session_id: 7,
            merchant_agent_id: 42,
            npc_model_id: 525,
            window_transaction_type: 0,
            transaction_service: 2,
            trophy_item_id: 900,
            required_item_model_id: 948,
            required_item_quantity: 4,
            reward_count: 1,
            captured_reward_count: 1,
            rewards_readable: true,
            ..CollectorOfferRecord::default()
        };
        record.reward_item_ids[0] = 901;
        record.reward_model_ids[0] = 12;
        record.reward_model_file_ids[0] = 0x8000_00CA;
        record.reward_item_types[0] = 2;
        record.reward_resolved[0] = true;
        let mut out = Vec::new();

        write_collector_offer_record(&mut out, &record).unwrap();

        let row: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(row["kind"], "collector_offers");
        assert_eq!(row["npc_model_id"], 525);
        assert_eq!(row["required_item"]["model_id"], 948);
        assert_eq!(row["required_item"]["quantity"], 4);
        assert_eq!(row["rewards"][0]["model_id"], 12);
        assert_eq!(row["rewards"][0]["model_file_id"], 202);
        assert_eq!(row["capture_complete"], true);

        record.npc_model_id = 0;
        out.clear();
        write_collector_offer_record(&mut out, &record).unwrap();
        let incomplete: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(incomplete["capture_complete"], false);
    }

    #[test]
    fn vendor_catalog_record_preserves_item_and_skill_ids() {
        let mut merchant = VendorCatalogRecord {
            ts_ms: 11,
            session_id: 7,
            merchant_agent_id: 42,
            npc_model_id: 525,
            transaction_service: 1,
            entry_count: 1,
            captured_entry_count: 1,
            entries_readable: true,
            ..VendorCatalogRecord::default()
        };
        merchant.entries[0] = VendorCatalogEntry {
            source_id: 901,
            model_id: 12,
            model_file_id: 0x8000_00CA,
            item_type: 2,
            base_value: 50,
            resolved: true,
            ..VendorCatalogEntry::default()
        };
        let mut out = Vec::new();
        write_vendor_catalog_record(&mut out, &merchant).unwrap();
        let row: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(row["kind"], "merchant_items");
        assert_eq!(row["entries"][0]["model_id"], 12);
        assert_eq!(row["entries"][0]["model_file_id"], 202);
        assert_eq!(row["entries"][0]["base_value"], 50);
        assert_eq!(row["capture_complete"], true);

        let mut trainer = VendorCatalogRecord {
            transaction_service: 10,
            ..merchant
        };
        trainer.entries[0] = VendorCatalogEntry {
            source_id: 900,
            aux: 1,
            ..VendorCatalogEntry::default()
        };
        out.clear();
        write_vendor_catalog_record(&mut out, &trainer).unwrap();
        let row: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(row["kind"], "skill_trainer_skills");
        assert_eq!(row["entries"][0]["skill_id"], 900);
        assert_eq!(row["entries"][0]["availability_flags_raw"], 1);
        assert_eq!(row["capture_complete"], true);
    }

    #[test]
    fn text_decode_id_record_writes_valid_json() {
        let mut record = DecodeIdRecord {
            ts_ms: 11,
            language_id: 2,
            encoded_len: 2,
            id_count: 1,
            ..DecodeIdRecord::default()
        };
        record.encoded[..2].copy_from_slice(&[0x0108, 0x0001]);
        record.ids[0] = 8;
        let mut out = Vec::new();

        write_decode_id_record(&mut out, &record).unwrap();

        let row: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(row["kind"], "text_decode_ids");
        assert_eq!(row["language_id"], 2);
        assert_eq!(row["encoded_hex"], "08010100");
        assert_eq!(row["decoded_ids"][0], 8);
    }

    #[test]
    fn item_record_embeds_matching_decode_ids() {
        let mut data = vec![0u8; 188];
        put_u32(&mut data, 0, GAME_SMSG_ITEM_GENERAL_INFO as u32);
        put_u32(&mut data, 4, 101);
        put_u32(&mut data, 8, 0x8000_00CA);
        put_u32(&mut data, 12, 3);
        put_u32(&mut data, 40, 9090);
        put_u32(&mut data, 44, 1);
        put_u16(&mut data, packet::ITEM_GENERAL_NAME_START, 0x21A8);

        let mut packet = PacketRecord {
            ts_ms: 1,
            session_id: 7,
            header: GAME_SMSG_ITEM_GENERAL_INFO as u32,
            len: data.len() as u16,
            from_handler: false,
            dispatch_arg0: 0,
            dispatch_arg2: 0,
            dispatch_arg3: 0,
            data: [0; MAX_PACKET_BYTES],
        };
        packet.data[..data.len()].copy_from_slice(&data);

        let mut decode = DecodeIdRecord {
            encoded_len: 2,
            id_count: 1,
            ..DecodeIdRecord::default()
        };
        decode.encoded[0] = 0x21A8;
        decode.encoded[1] = 0;
        decode.ids[0] = 8360;
        let mut decodes = BTreeMap::new();
        decodes.insert(decode_id_key(&decode), decode);

        let mut out = Vec::new();
        write_record(&mut out, &packet, &decodes).unwrap();
        let row: Value = serde_json::from_slice(&out).unwrap();

        assert_eq!(row.get("kind"), None);
        assert_eq!(row["item_id"], 101);
        assert_eq!(row["decoded_ids"][0], 8360);
        assert_eq!(row["text_refs"][0]["text_file_index"], 8);
        assert_eq!(row["text_refs"][0]["record_index"], 168);
    }

    #[test]
    fn text_decode_trace_record_writes_valid_json() {
        let mut record = TextDecodeTraceRecord::new();
        record.ts_ms = 123;
        record.has_ref = false;
        record.language_id = 2;
        record.context = 0x1234;
        record.record_ptr = 0x2000;
        record.record_size = 4;
        record.compression_or_flags = 0x80;
        record.record_type = 0x12;
        record.record_subtype = 0x34;
        record.record_bytes_len = 4;
        record.record_truncated = true;
        record.record_bytes[..4].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        record.output_ptr = 0x3000;
        record.output_len = 5;
        record.output_truncated = false;
        record.output[..5].copy_from_slice(&[0x0041, 0x0022, 0x000a, 0xd83d, 0xde80]);
        record.substitute_start = 0x4000;
        record.substitute_end = 0x4006;
        record.substitute_len = 3;
        record.substitute_truncated = true;
        record.substitute[..3].copy_from_slice(&[0x0009, 0x0062, 0x263a]);

        let mut out = Vec::new();
        write_text_trace_record(&mut out, &record).unwrap();

        let row: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(row["kind"], "text_decode_trace");
        assert_eq!(row["ts_ms"], 123);
        assert_eq!(row["language_id"], 2);
        assert_eq!(row["context"], 0x1234);
        assert_eq!(row["record_ptr"], 0x2000);
        assert_eq!(row["record_size"], 4);
        assert_eq!(row["compression_or_flags"], 0x80);
        assert_eq!(row["record_type"], 0x12);
        assert_eq!(row["record_subtype"], 0x34);
        assert_eq!(row["ref_language_id"], Value::Null);
        assert_eq!(row["decoded_id"], Value::Null);
        assert_eq!(row["text_file_index"], Value::Null);
        assert_eq!(row["record_index"], Value::Null);
        assert_eq!(row["ref_age_ms"], Value::Null);
        assert_eq!(row["record_truncated"], true);
        assert_eq!(row["record_hex"], "deadbeef");
        assert_eq!(row["output_ptr"], 0x3000);
        assert_eq!(row["output_len"], 5);
        assert_eq!(row["output_truncated"], false);
        assert_eq!(row["output_u16_hex"], "410022000a003dd880de");
        assert_eq!(row["output_preview"], "A\"\n🚀");
        assert_eq!(row["substitute_start"], 0x4000);
        assert_eq!(row["substitute_end"], 0x4006);
        assert_eq!(row["substitute_len"], 3);
        assert_eq!(row["substitute_truncated"], true);
        assert_eq!(row["substitute_u16_hex"], "090062003a26");
    }

    fn put_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
}
