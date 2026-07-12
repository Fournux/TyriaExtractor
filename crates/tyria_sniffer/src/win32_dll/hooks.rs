use super::*;

#[no_mangle]
pub unsafe extern "C" fn gwdb_on_stoc_packet(packet: *const u8, stack_size: u32) {
    if !readable(packet, size_of::<u32>()) {
        return;
    }

    let header = ptr::read_unaligned(packet.cast::<u32>());
    let guessed = packet::guessed_packet_size(header);
    let len = if (4..=MAX_PACKET_BYTES as u32).contains(&stack_size) {
        stack_size as usize
    } else {
        guessed
    }
    .min(MAX_PACKET_BYTES);
    if !readable(packet, len) {
        return;
    }

    let mut record = PacketRecord {
        ts_ms: now_ms(),
        session_id: capture_session_id(),
        header,
        len: len as u16,
        from_handler: false,
        dispatch_arg0: 0,
        dispatch_arg2: 0,
        dispatch_arg3: 0,
        data: [0; MAX_PACKET_BYTES],
    };
    ptr::copy_nonoverlapping(packet, record.data.as_mut_ptr(), len);

    push_bounded(&RING, record);
}

unsafe extern "C" fn item_general_handler(
    arg0: usize,
    packet: *mut u8,
    arg2: usize,
    arg3: usize,
) -> bool {
    let result = call_original_handler(&ORIGINAL_ITEM_GENERAL, arg0, packet, arg2, arg3);
    record_handler_packet(arg0, packet, arg2, arg3);
    result
}

unsafe extern "C" fn item_reuse_handler(
    arg0: usize,
    packet: *mut u8,
    arg2: usize,
    arg3: usize,
) -> bool {
    let result = call_original_handler(&ORIGINAL_ITEM_REUSE, arg0, packet, arg2, arg3);
    record_handler_packet(arg0, packet, arg2, arg3);
    result
}

macro_rules! quest_handler {
    ($name:ident, $index:expr) => {
        unsafe extern "C" fn $name(arg0: usize, packet: *mut u8, arg2: usize, arg3: usize) -> bool {
            record_quest_handler_packet($index, arg0, packet, arg2, arg3);
            call_original_handler(&ORIGINAL_QUEST_HANDLERS[$index], arg0, packet, arg2, arg3)
        }
    };
}

quest_handler!(agent_spawned_handler, 0);
quest_handler!(agent_despawned_handler, 1);
quest_handler!(quest_add_handler, 2);
quest_handler!(quest_description_handler, 3);
quest_handler!(quest_general_info_handler, 4);
quest_handler!(quest_update_marker_handler, 5);
quest_handler!(quest_remove_handler, 6);
quest_handler!(quest_add_marker_handler, 7);
quest_handler!(quest_update_objectives_handler, 8);
quest_handler!(npc_update_properties_handler, 9);
quest_handler!(dialog_button_handler, 10);
quest_handler!(dialog_sender_handler, 11);
quest_handler!(instance_load_info_handler, 12);

const QUEST_REPLACEMENT_HANDLERS: [StoCHandlerFn; QUEST_PACKET_HEADERS.len()] = [
    agent_spawned_handler,
    agent_despawned_handler,
    quest_add_handler,
    quest_description_handler,
    quest_general_info_handler,
    quest_update_marker_handler,
    quest_remove_handler,
    quest_add_marker_handler,
    quest_update_objectives_handler,
    npc_update_properties_handler,
    dialog_button_handler,
    dialog_sender_handler,
    instance_load_info_handler,
];

unsafe fn record_quest_handler_packet(
    index: usize,
    arg0: usize,
    packet: *mut u8,
    arg2: usize,
    arg3: usize,
) {
    let expected_header = QUEST_PACKET_HEADERS[index] as u32;
    let expected_len = quest_packet_size(expected_header);
    for candidate in [
        packet.cast_const(),
        arg0 as *const u8,
        arg2 as *const u8,
        arg3 as *const u8,
    ] {
        let Some(packet) = find_packet_header(candidate, expected_header) else {
            continue;
        };
        if !readable(packet, expected_len) {
            continue;
        }
        let bytes = std::slice::from_raw_parts(packet, expected_len);
        if !valid_quest_capture_packet(expected_header, bytes) {
            continue;
        }
        let mut record = QuestPacketRecord {
            ts_ms: now_ms(),
            session_id: capture_session_id(),
            header: expected_header,
            len: expected_len as u16,
            data: [0; MAX_QUEST_PACKET_BYTES],
        };
        record.data[..expected_len].copy_from_slice(bytes);
        push_quest_packet(record);
        return;
    }
}

fn quest_packet_size(header: u32) -> usize {
    match header {
        0x20 => 0x74,
        0x21 => 8,
        0x49 => 0x50,
        0x4c => 0x208,
        0x50 => 0x40,
        0x51 | 0x53 => 0x18,
        0x52 => 8,
        0x54 => 0x108,
        0x56 => 0x34,
        0x7e => 0x110,
        0x81 => 8,
        0x199 => 0x1c,
        _ => 0,
    }
}

fn fixed_schema_packet_size(schema: &QuestPacketSchemaRecord) -> Option<usize> {
    let field_count = schema.field_count as usize;
    if field_count == 0 || field_count > schema.fields.len() {
        return None;
    }
    schema.fields[1..field_count]
        .iter()
        .try_fold(size_of::<u32>(), |size, descriptor| {
            let count = ((descriptor >> 8) & 0xffff) as usize;
            let field_size = match descriptor & 0xf {
                0 | 1 | 4 => 4,
                2 => 8,
                7 => count.checked_mul(2)?,
                _ => return None,
            };
            size.checked_add(field_size)
        })
}

fn valid_quest_capture_packet(header: u32, bytes: &[u8]) -> bool {
    let u32_at = |offset| {
        bytes
            .get(offset..offset + 4)
            .and_then(|raw| raw.try_into().ok())
            .map(u32::from_le_bytes)
    };
    match header {
        0x20 => {
            u32_at(4).is_some_and(|agent_id| agent_id != 0)
                && u32_at(8).is_some_and(|agent_type| agent_type & 0xf000_0000 == 0x2000_0000)
        }
        0x21 | 0x81 => u32_at(4).is_some_and(|agent_id| agent_id != 0),
        0x49 | 0x4c | 0x50 | 0x51 | 0x52 | 0x53 | 0x54 => {
            u32_at(4).is_some_and(|quest_id| (1..=u32::from(u16::MAX)).contains(&quest_id))
        }
        0x56 => u32_at(4).is_some_and(|model_id| !matches!(model_id, 0 | u32::MAX)),
        0x7e => u32_at(264).is_some_and(|dialog_id| {
            dialog_id & 0x0080_0000 != 0
                && (1..=u32::from(u16::MAX)).contains(&((dialog_id ^ 0x0080_0000) >> 8))
                && matches!(dialog_id & 0xf, 1..=7)
        }),
        0x199 => u32_at(8).is_some_and(|map_id| map_id != 0),
        _ => false,
    }
}

unsafe fn record_handler_packet(arg0: usize, packet: *mut u8, arg2: usize, arg3: usize) {
    let candidates = [
        packet.cast::<u8>(),
        arg0 as *const u8,
        arg2 as *const u8,
        arg3 as *const u8,
    ];

    for candidate in candidates {
        if let Some((packet, header)) = find_stoc_packet(candidate) {
            record_item_packet(packet, header, arg0, arg2, arg3);
            return;
        }
    }

    if let Some(probe) = candidates
        .into_iter()
        .find(|candidate| readable(*candidate, 64))
    {
        let header = ptr::read_unaligned(probe.cast::<u32>());
        record_item_packet(probe, header, arg0, arg2, arg3);
    }
}

unsafe fn record_item_packet(
    packet: *const u8,
    header: u32,
    arg0: usize,
    arg2: usize,
    arg3: usize,
) {
    let guessed = packet::guessed_packet_size(header);
    let len = guessed.min(MAX_PACKET_BYTES);
    if !readable(packet, len) {
        return;
    }
    let mut record = PacketRecord {
        ts_ms: now_ms(),
        session_id: capture_session_id(),
        header,
        len: len as u16,
        from_handler: true,
        dispatch_arg0: arg0,
        dispatch_arg2: arg2,
        dispatch_arg3: arg3,
        data: [0; MAX_PACKET_BYTES],
    };
    ptr::copy_nonoverlapping(packet, record.data.as_mut_ptr(), len);

    push_bounded(&RING, record);
}

unsafe fn call_original_handler(
    original: &AtomicUsize,
    arg0: usize,
    packet: *mut u8,
    arg2: usize,
    arg3: usize,
) -> bool {
    let original = original.load(Ordering::Acquire);
    if original == 0 {
        return true;
    }
    let handler: StoCHandlerFn = std::mem::transmute(original);
    handler(arg0, packet, arg2, arg3)
}

unsafe extern "C" fn gwdb_on_text_decode_ids(context: *const c_void, parser: *const c_void) {
    let Some(count) = read_unaligned_at::<u32>(context, 0x168).map(|count| count as usize) else {
        return;
    };
    if count == 0 || count > 1024 {
        return;
    }

    let Some(ids_ptr) = read_unaligned_at::<*const u32>(context, 0x160) else {
        return;
    };
    let id_count = count.min(MAX_CAPTURED_DECODE_IDS);
    if ids_ptr.is_null() || !readable(ids_ptr.cast::<u8>(), id_count * size_of::<u32>()) {
        return;
    }

    let Some(encoded_ptr) = read_unaligned_at::<*const u16>(parser, 0x0c) else {
        return;
    };
    let Some(encoded_len) =
        read_unaligned_at::<u32>(parser, 0x14).map(|encoded_len| encoded_len as usize)
    else {
        return;
    };
    if encoded_ptr.is_null()
        || encoded_len == 0
        || encoded_len > MAX_CAPTURED_ITEM_STRING_U16
        || !readable(encoded_ptr.cast::<u8>(), encoded_len * size_of::<u16>())
    {
        return;
    }
    let Some(language_id) = read_unaligned_at::<u32>(parser, 0x2c) else {
        return;
    };

    let mut record = DecodeIdRecord {
        ts_ms: now_ms(),
        language_id,
        encoded_len: encoded_len as u16,
        ..DecodeIdRecord::default()
    };
    ptr::copy_nonoverlapping(encoded_ptr, record.encoded.as_mut_ptr(), encoded_len);
    record.id_count = id_count as u16;
    ptr::copy_nonoverlapping(ids_ptr, record.ids.as_mut_ptr(), id_count);

    push_bounded(&DECODE_ID_RING, record);
}

unsafe extern "C" fn gwdb_on_text_resource_ref(
    language_id: u32,
    decoded_id: u32,
    language_files: *const c_void,
    text_file_index: u32,
    record_index: u32,
) {
    let Some(file_array) = read_unaligned_at::<*const u8>(language_files, 0x34) else {
        return;
    };
    if file_array.is_null() {
        return;
    }
    let Some(offset) = (text_file_index as usize).checked_mul(TEXT_RESOURCE_DESC_BYTES) else {
        return;
    };
    let Some(file_desc) = (file_array as usize)
        .checked_add(offset)
        .map(|address| address as *const u8)
    else {
        return;
    };
    if !readable(file_desc, TEXT_RESOURCE_DESC_BYTES) {
        return;
    }

    let mut record = TextResourceRefRecord {
        ts_ms: now_ms(),
        language_id,
        decoded_id,
        text_file_index,
        record_index,
        file_desc: [0; TEXT_RESOURCE_DESC_BYTES],
    };
    ptr::copy_nonoverlapping(
        file_desc,
        record.file_desc.as_mut_ptr(),
        TEXT_RESOURCE_DESC_BYTES,
    );
    if let Ok(mut last) = LAST_TEXT_RESOURCE_REF.try_lock() {
        *last = Some(TextResourceRefSnapshot {
            ts_ms: record.ts_ms,
            language_id,
            decoded_id,
            text_file_index,
            record_index,
        });
    }

    push_bounded(&TEXT_RESOURCE_REF_RING, record);
}

unsafe extern "system" fn text_record_decode_hook(
    context: *mut c_void,
    data: *const u8,
    substitute_start: *const u16,
    substitute_end: *const u16,
) -> *const u16 {
    let original: TextRecordDecodeFn =
        std::mem::transmute(ORIGINAL_TEXT_RECORD_DECODE.load(Ordering::Acquire));
    let output = original(context, data, substitute_start, substitute_end);
    record_text_decode_trace(context, data, substitute_start, substitute_end, output);
    output
}

unsafe fn record_text_decode_trace(
    context: *mut c_void,
    data: *const u8,
    substitute_start: *const u16,
    substitute_end: *const u16,
    output: *const u16,
) {
    if context.is_null() || data.is_null() || !readable(data, 6) {
        return;
    }
    let record_size = ptr::read_unaligned(data.cast::<u16>());
    if !(6..=4096).contains(&record_size) {
        return;
    }
    let record_len = (record_size as usize).min(MAX_TEXT_TRACE_RECORD_BYTES);
    if !readable(data, record_len) {
        return;
    }

    let now = now_ms();
    let mut record = TextDecodeTraceRecord::new();
    record.ts_ms = now;
    record.context = context as usize;
    record.record_ptr = data as usize;
    record.record_size = record_size;
    record.compression_or_flags = ptr::read_unaligned(data.add(2).cast::<u16>());
    record.record_type = ptr::read_unaligned(data.add(4));
    record.record_subtype = ptr::read_unaligned(data.add(5));
    record.record_bytes_len = record_len as u16;
    record.record_truncated = record_size as usize > record_len;
    ptr::copy_nonoverlapping(data, record.record_bytes.as_mut_ptr(), record_len);

    if let Some(language_id) = read_unaligned_at::<u32>(context.cast_const().cast(), 0x1d0) {
        record.language_id = language_id;
    }

    let output_ptr = if output.is_null() {
        read_unaligned_at::<*const u16>(context.cast_const().cast(), 0x170).unwrap_or(ptr::null())
    } else {
        output
    };
    if !output_ptr.is_null() {
        if let Some(output_count) = read_unaligned_at::<u32>(context.cast_const().cast(), 0x178)
            .map(|output_count| output_count as usize)
        {
            let output_len = output_count.min(MAX_TEXT_TRACE_U16);
            if output_len > 0 && readable(output_ptr.cast::<u8>(), output_len * size_of::<u16>()) {
                record.output_ptr = output_ptr as usize;
                record.output_len = output_len as u16;
                record.output_truncated = output_count > output_len;
                ptr::copy_nonoverlapping(output_ptr, record.output.as_mut_ptr(), output_len);
            }
        }
    }

    if !substitute_start.is_null()
        && !substitute_end.is_null()
        && substitute_end as usize >= substitute_start as usize
    {
        let substitute_count =
            (substitute_end as usize - substitute_start as usize) / std::mem::size_of::<u16>();
        let substitute_len = substitute_count.min(MAX_TEXT_TRACE_U16);
        if substitute_len > 0 && readable(substitute_start.cast::<u8>(), substitute_len * 2) {
            record.substitute_start = substitute_start as usize;
            record.substitute_end = substitute_end as usize;
            record.substitute_len = substitute_len as u16;
            record.substitute_truncated = substitute_count > substitute_len;
            ptr::copy_nonoverlapping(
                substitute_start,
                record.substitute.as_mut_ptr(),
                substitute_len,
            );
        }
    }

    if let Ok(last) = LAST_TEXT_RESOURCE_REF.try_lock() {
        if let Some(last) = *last {
            record.has_ref = true;
            record.ref_language_id = last.language_id;
            record.ref_decoded_id = last.decoded_id;
            record.ref_text_file_index = last.text_file_index;
            record.ref_record_index = last.record_index;
            record.ref_age_ms = now.saturating_sub(last.ts_ms).min(u128::from(u32::MAX)) as u32;
        }
    }

    push_bounded(&TEXT_TRACE_RING, record);
}

unsafe fn stoc_handler_array() -> Result<*mut GwArray<StoCHandler>, &'static str> {
    let base = GetModuleHandleA(ptr::null());
    if base.is_null() {
        return Err("module_base_not_found");
    }
    let (text_start, text_len) = pe_text_section(base as usize).ok_or("text_section_not_found")?;
    let pattern_addr = find_pattern(text_start, text_len, STOC_HANDLER_ARRAY_PATTERN)
        .ok_or("stoc_handler_array_pattern_not_found")?;
    let game_server_global_ptr = pattern_addr
        .checked_sub(6)
        .ok_or("stoc_game_server_global_not_found")?;
    let game_server_global_addr =
        read_unaligned_at::<usize>(game_server_global_ptr as *const c_void, 0)
            .ok_or("stoc_game_server_global_not_found")?;
    if game_server_global_addr == 0 {
        return Err("stoc_game_server_global_not_found");
    }
    let game_server =
        read_unaligned_at::<*mut GameServer>(game_server_global_addr as *const c_void, 0)
            .ok_or("stoc_game_server_not_ready")?;
    if game_server.is_null() || !readable(game_server.cast::<u8>(), size_of::<GameServer>()) {
        return Err("stoc_game_server_not_ready");
    }
    let codec = (*game_server).gs_codec;
    if codec.is_null() || !readable(codec.cast::<u8>(), size_of::<GameServerCodec>()) {
        return Err("stoc_game_server_codec_not_ready");
    }
    Ok(&mut (*codec).handlers)
}

pub(super) unsafe fn install_stoc_handler_hooks() -> Result<usize, &'static str> {
    let handlers = &mut *stoc_handler_array()?;
    let required_handlers = GAME_SMSG_ITEM_REUSE_ID + 1;
    if handlers.buffer.is_null()
        || (handlers.size as usize) < required_handlers
        || (handlers.capacity as usize) < required_handlers
    {
        return Err("stoc_handler_array_not_ready");
    }

    let item_general = handlers.buffer.add(GAME_SMSG_ITEM_GENERAL_INFO);
    let item_reuse = handlers.buffer.add(GAME_SMSG_ITEM_REUSE_ID);
    let item_general_replacement = item_general_handler as *const () as usize;
    let item_reuse_replacement = item_reuse_handler as *const () as usize;
    let item_general_original = handler_original(item_general, item_general_replacement)?;
    let item_reuse_original = handler_original(item_reuse, item_reuse_replacement)?;

    ORIGINAL_ITEM_GENERAL.store(item_general_original, Ordering::Release);
    ORIGINAL_ITEM_REUSE.store(item_reuse_original, Ordering::Release);
    (*item_general).handler_func = item_general_replacement;
    (*item_reuse).handler_func = item_reuse_replacement;

    Ok(handlers.buffer as usize)
}

pub(super) unsafe fn install_quest_handler_hooks(
) -> Result<(usize, [QuestPacketSchemaRecord; QUEST_PACKET_HEADERS.len()]), &'static str> {
    let handlers = &mut *stoc_handler_array()?;
    let required_handlers = QUEST_PACKET_HEADERS[QUEST_PACKET_HEADERS.len() - 1] + 1;
    if handlers.buffer.is_null()
        || (handlers.size as usize) < required_handlers
        || (handlers.capacity as usize) < required_handlers
    {
        return Err("quest_handler_array_not_ready");
    }

    let mut originals = [0_usize; QUEST_PACKET_HEADERS.len()];
    let mut schemas = [QuestPacketSchemaRecord {
        header: 0,
        field_count: 0,
        fields: [0; MAX_QUEST_SCHEMA_FIELDS],
    }; QUEST_PACKET_HEADERS.len()];
    for (index, &header) in QUEST_PACKET_HEADERS.iter().enumerate() {
        let handler = handlers.buffer.add(header);
        let replacement = QUEST_REPLACEMENT_HANDLERS[index] as *const () as usize;
        originals[index] = handler_original(handler, replacement)?;
        let field_count = (*handler).field_count as usize;
        if field_count == 0
            || field_count > MAX_QUEST_SCHEMA_FIELDS
            || (*handler).fields.is_null()
            || !readable(
                (*handler).fields.cast::<u8>(),
                field_count * size_of::<u32>(),
            )
        {
            return Err("quest_handler_schema_not_ready");
        }
        schemas[index].header = header as u32;
        schemas[index].field_count = field_count as u16;
        ptr::copy_nonoverlapping(
            (*handler).fields,
            schemas[index].fields.as_mut_ptr(),
            field_count,
        );
        if fixed_schema_packet_size(&schemas[index]) != Some(quest_packet_size(header as u32)) {
            return Err("quest_handler_schema_changed");
        }
    }

    for (index, &header) in QUEST_PACKET_HEADERS.iter().enumerate() {
        let handler = handlers.buffer.add(header);
        ORIGINAL_QUEST_HANDLERS[index].store(originals[index], Ordering::Release);
        (*handler).handler_func = QUEST_REPLACEMENT_HANDLERS[index] as *const () as usize;
    }

    Ok((handlers.buffer as usize, schemas))
}

pub(super) unsafe fn install_text_decode_id_hook() -> Result<usize, &'static str> {
    let base = GetModuleHandleA(ptr::null());
    if base.is_null() {
        return Err("module_base_not_found");
    }
    let (text_start, text_len) = pe_text_section(base as usize).ok_or("text_section_not_found")?;
    let hook_addr = find_pattern(text_start, text_len, TEXT_DECODE_IDS_HOOK_PATTERN)
        .ok_or("text_decode_ids_pattern_not_found")?;
    let expected = std::slice::from_raw_parts(hook_addr as *const u8, TEXT_DECODE_IDS_HOOK_LEN);
    if expected != &TEXT_DECODE_IDS_HOOK_PATTERN[..TEXT_DECODE_IDS_HOOK_LEN] {
        return Err("text_decode_ids_hook_unexpected_bytes");
    }
    let trampoline = build_text_decode_id_trampoline(hook_addr)?;
    write_jmp_len(hook_addr, trampoline, TEXT_DECODE_IDS_HOOK_LEN)?;
    Ok(hook_addr)
}

pub(super) unsafe fn install_text_resource_ref_hook() -> Result<usize, &'static str> {
    let base = GetModuleHandleA(ptr::null());
    if base.is_null() {
        return Err("module_base_not_found");
    }
    let (text_start, text_len) = pe_text_section(base as usize).ok_or("text_section_not_found")?;
    let hook_addr = find_pattern(text_start, text_len, TEXT_RESOURCE_REF_HOOK_PATTERN)
        .ok_or("text_resource_ref_pattern_not_found")?;
    let expected = std::slice::from_raw_parts(hook_addr as *const u8, TEXT_RESOURCE_REF_HOOK_LEN);
    if expected != &TEXT_RESOURCE_REF_HOOK_PATTERN[..TEXT_RESOURCE_REF_HOOK_LEN] {
        return Err("text_resource_ref_hook_unexpected_bytes");
    }
    let trampoline = build_text_resource_ref_trampoline(hook_addr)?;
    write_jmp_len(hook_addr, trampoline, TEXT_RESOURCE_REF_HOOK_LEN)?;
    Ok(hook_addr)
}

pub(super) unsafe fn install_text_record_decode_hook() -> Result<usize, &'static str> {
    let base = GetModuleHandleA(ptr::null());
    if base.is_null() {
        return Err("module_base_not_found");
    }
    let (text_start, text_len) = pe_text_section(base as usize).ok_or("text_section_not_found")?;
    let hook_addr = find_pattern(text_start, text_len, TEXT_RECORD_DECODE_PATTERN)
        .ok_or("text_record_decode_pattern_not_found")?;
    let expected = std::slice::from_raw_parts(hook_addr as *const u8, TEXT_RECORD_DECODE_HOOK_LEN);
    if expected != &TEXT_RECORD_DECODE_PATTERN[..TEXT_RECORD_DECODE_HOOK_LEN] {
        return Err("text_record_decode_hook_unexpected_bytes");
    }
    let trampoline = build_function_trampoline(hook_addr, TEXT_RECORD_DECODE_HOOK_LEN)?;
    ORIGINAL_TEXT_RECORD_DECODE.store(trampoline, Ordering::Release);
    write_jmp_len(
        hook_addr,
        text_record_decode_hook as *const () as usize,
        TEXT_RECORD_DECODE_HOOK_LEN,
    )?;
    Ok(hook_addr)
}

unsafe fn build_function_trampoline(hook_addr: usize, len: usize) -> Result<usize, &'static str> {
    let mem = VirtualAlloc(
        ptr::null(),
        len + 16,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    ) as *mut u8;
    if mem.is_null() {
        return Err("function_trampoline_alloc_failed");
    }

    let base = mem as usize;
    let mut code = Vec::with_capacity(len + 5);
    code.extend_from_slice(std::slice::from_raw_parts(hook_addr as *const u8, len));
    emit_rel_jmp(&mut code, base, hook_addr + len)?;
    ptr::copy_nonoverlapping(code.as_ptr(), mem, code.len());
    FlushInstructionCache(GetCurrentProcess(), mem.cast(), code.len());
    Ok(base)
}

unsafe fn build_text_resource_ref_trampoline(hook_addr: usize) -> Result<usize, &'static str> {
    let mem = VirtualAlloc(
        ptr::null(),
        96,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    ) as *mut u8;
    if mem.is_null() {
        return Err("text_resource_ref_trampoline_alloc_failed");
    }

    let base = mem as usize;
    let mut code = Vec::with_capacity(48);
    code.extend_from_slice(&[0x9C, 0x60]);
    code.push(0x52);
    code.push(0x56);
    code.push(0x53);
    code.extend_from_slice(&[0xFF, 0x75, 0x0C]);
    code.extend_from_slice(&[0xFF, 0x75, 0x08]);
    emit_rel_call(
        &mut code,
        base,
        gwdb_on_text_resource_ref as *const () as usize,
    )?;
    code.extend_from_slice(&[0x83, 0xC4, 0x14, 0x61, 0x9D]);
    code.extend_from_slice(&TEXT_RESOURCE_REF_HOOK_PATTERN[..TEXT_RESOURCE_REF_HOOK_LEN]);
    emit_rel_jmp(&mut code, base, hook_addr + TEXT_RESOURCE_REF_HOOK_LEN)?;

    ptr::copy_nonoverlapping(code.as_ptr(), mem, code.len());
    FlushInstructionCache(GetCurrentProcess(), mem.cast(), code.len());
    Ok(base)
}

unsafe fn build_text_decode_id_trampoline(hook_addr: usize) -> Result<usize, &'static str> {
    let mem = VirtualAlloc(
        ptr::null(),
        64,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    ) as *mut u8;
    if mem.is_null() {
        return Err("text_decode_ids_trampoline_alloc_failed");
    }

    let base = mem as usize;
    let mut code = Vec::with_capacity(32);
    code.extend_from_slice(&[0x9C, 0x60, 0x56, 0x53]);
    emit_rel_call(
        &mut code,
        base,
        gwdb_on_text_decode_ids as *const () as usize,
    )?;
    code.extend_from_slice(&[0x83, 0xC4, 0x08, 0x61, 0x9D]);
    code.extend_from_slice(&TEXT_DECODE_IDS_HOOK_PATTERN[..TEXT_DECODE_IDS_HOOK_LEN]);
    emit_rel_jmp(&mut code, base, hook_addr + TEXT_DECODE_IDS_HOOK_LEN)?;

    ptr::copy_nonoverlapping(code.as_ptr(), mem, code.len());
    FlushInstructionCache(GetCurrentProcess(), mem.cast(), code.len());
    Ok(base)
}

fn emit_rel_call(code: &mut Vec<u8>, base: usize, dst: usize) -> Result<(), &'static str> {
    emit_rel32(code, base, 0xE8, dst)
}

fn emit_rel_jmp(code: &mut Vec<u8>, base: usize, dst: usize) -> Result<(), &'static str> {
    emit_rel32(code, base, 0xE9, dst)
}

fn emit_rel32(code: &mut Vec<u8>, base: usize, op: u8, dst: usize) -> Result<(), &'static str> {
    let src = base + code.len();
    let rel = (dst as isize)
        .checked_sub(src as isize)
        .and_then(|value| value.checked_sub(5))
        .ok_or("relative_address_overflow")?;
    if rel < i32::MIN as isize || rel > i32::MAX as isize {
        return Err("relative_address_out_of_i32_range");
    }
    code.push(op);
    code.extend_from_slice(&(rel as i32).to_le_bytes());
    Ok(())
}

unsafe fn write_jmp_len(src: usize, dst: usize, len: usize) -> Result<(), &'static str> {
    if len < 5 {
        return Err("jmp_patch_too_short");
    }
    let rel = (dst as isize)
        .checked_sub(src as isize)
        .and_then(|value| value.checked_sub(5))
        .ok_or("jmp_range_overflow")?;
    if rel < i32::MIN as isize || rel > i32::MAX as isize {
        return Err("jmp_out_of_i32_range");
    }

    let mut old = 0u32;
    if VirtualProtect(src as *const c_void, len, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
        return Err("virtualprotect_failed");
    }
    let patch = src as *mut u8;
    ptr::write(patch, 0xE9);
    ptr::write_unaligned(patch.add(1).cast::<i32>(), rel as i32);
    for offset in 5..len {
        ptr::write(patch.add(offset), 0x90);
    }
    let mut ignored = 0u32;
    let _ = VirtualProtect(src as *const c_void, len, old, &mut ignored);
    FlushInstructionCache(GetCurrentProcess(), src as *const c_void, len);
    Ok(())
}

unsafe fn handler_original(
    handler: *mut StoCHandler,
    replacement: usize,
) -> Result<usize, &'static str> {
    if handler.is_null() || !readable(handler.cast::<u8>(), size_of::<StoCHandler>()) {
        return Err("stoc_handler_entry_not_ready");
    }
    let original = (*handler).handler_func;
    if original == 0 {
        return Err("stoc_handler_func_not_ready");
    }
    if original == replacement {
        return Err("stoc_handler_already_installed");
    }
    Ok(original)
}

unsafe fn find_stoc_packet(candidate: *const u8) -> Option<(*const u8, u32)> {
    for header in [GAME_SMSG_ITEM_GENERAL_INFO, GAME_SMSG_ITEM_REUSE_ID] {
        if let Some(packet) = find_packet_header(candidate, header as u32) {
            return Some((packet, header as u32));
        }
    }
    None
}

unsafe fn find_packet_header(candidate: *const u8, expected_header: u32) -> Option<*const u8> {
    let readable_len = readable_prefix(candidate, 64);
    if readable_len < size_of::<u32>() {
        return None;
    }
    for offset in (0..=readable_len - size_of::<u32>()).step_by(4) {
        let packet = candidate.wrapping_add(offset);
        if ptr::read_unaligned(packet.cast::<u32>()) == expected_header {
            return Some(packet);
        }
    }
    None
}

pub(super) unsafe fn readable(ptr: *const u8, len: usize) -> bool {
    readable_prefix(ptr, len) == len
}

pub(super) unsafe fn readable_prefix(ptr: *const u8, len: usize) -> usize {
    if ptr.is_null() || len == 0 {
        return 0;
    }
    let start = ptr as usize;
    let Some(end) = start.checked_add(len) else {
        return 0;
    };
    let mut cursor = start;
    while cursor < end {
        let mut info = std::mem::zeroed::<MEMORY_BASIC_INFORMATION>();
        if VirtualQuery(
            cursor as *const c_void,
            &mut info,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        ) == 0
        {
            break;
        }
        if info.State != MEM_COMMIT || !is_readable_protection(info.Protect) {
            break;
        }
        let base = info.BaseAddress as usize;
        let Some(region_end) = base.checked_add(info.RegionSize) else {
            break;
        };
        if cursor < base || region_end <= cursor {
            break;
        }
        cursor = region_end.min(end);
    }
    cursor - start
}

fn is_readable_protection(protection: u32) -> bool {
    if protection & (PAGE_NOACCESS | PAGE_GUARD) != 0 {
        return false;
    }
    matches!(
        protection & 0xff,
        PAGE_READONLY
            | PAGE_READWRITE
            | PAGE_WRITECOPY
            | PAGE_EXECUTE_READ
            | PAGE_EXECUTE_READWRITE
            | PAGE_EXECUTE_WRITECOPY
    )
}

unsafe fn read_unaligned_at<T: Copy>(base: *const c_void, offset: usize) -> Option<T> {
    let address = (base as usize).checked_add(offset)? as *const u8;
    if !readable(address, size_of::<T>()) {
        return None;
    }
    Some(ptr::read_unaligned(address.cast::<T>()))
}

pub(super) unsafe fn client_pe_timestamp() -> Option<u32> {
    let base = GetModuleHandleA(ptr::null()) as usize;
    if base == 0 || read_u16(base, 0)? != 0x5a4d {
        return None;
    }
    let pe_offset = read_u32(base, 0x3c)? as usize;
    let timestamp_offset = pe_offset.checked_add(8)?;
    if read_u32(base, pe_offset)? != 0x0000_4550 {
        return None;
    }
    read_u32(base, timestamp_offset)
}

unsafe fn pe_text_section(base: usize) -> Option<(usize, usize)> {
    if read_u16(base, 0)? != 0x5A4D {
        return None;
    }
    let pe_off = read_u32(base, 0x3C)? as usize;
    if read_u32(base, pe_off)? != 0x0000_4550 {
        return None;
    }
    let section_count = read_u16(base, pe_off.checked_add(6)?)? as usize;
    let optional_header_size = read_u16(base, pe_off.checked_add(20)?)? as usize;
    let section_table = pe_off.checked_add(24)?.checked_add(optional_header_size)?;
    for index in 0..section_count {
        let section = section_table.checked_add(index.checked_mul(40)?)?;
        let name_address = base.checked_add(section)? as *const u8;
        if !readable(name_address, 8) {
            return None;
        }
        let name = std::slice::from_raw_parts(name_address, 8);
        let nul = name
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(name.len());
        if &name[..nul] != b".text" {
            continue;
        }
        let virtual_size = read_u32(base, section.checked_add(8)?)? as usize;
        let virtual_address = read_u32(base, section.checked_add(12)?)? as usize;
        let raw_size = read_u32(base, section.checked_add(16)?)? as usize;
        return Some((
            base.checked_add(virtual_address)?,
            virtual_size.max(raw_size),
        ));
    }
    None
}

unsafe fn read_u16(base: usize, offset: usize) -> Option<u16> {
    read_unaligned_at(base as *const c_void, offset)
}

unsafe fn read_u32(base: usize, offset: usize) -> Option<u32> {
    read_unaligned_at(base as *const c_void, offset)
}

unsafe fn find_pattern(start: usize, len: usize, pattern: &[u8]) -> Option<usize> {
    if pattern.is_empty() || len < pattern.len() {
        return None;
    }
    let start_ptr = start as *const u8;
    if !readable(start_ptr, len) {
        return None;
    }
    let haystack = std::slice::from_raw_parts(start_ptr, len);
    haystack
        .windows(pattern.len())
        .position(|window| window == pattern)
        .and_then(|offset| start.checked_add(offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Memory::{VirtualFree, MEM_RELEASE};

    #[test]
    fn guarded_pe_reads_reject_invalid_offsets() {
        let mut image = vec![0_u8; 0x200];
        image[0..2].copy_from_slice(&0x5A4D_u16.to_le_bytes());
        image[0x3c..0x40].copy_from_slice(&0x40_u32.to_le_bytes());
        image[0x40..0x44].copy_from_slice(&0x0000_4550_u32.to_le_bytes());
        image[0x46..0x48].copy_from_slice(&1_u16.to_le_bytes());
        image[0x54..0x56].copy_from_slice(&0_u16.to_le_bytes());
        image[0x58..0x60].copy_from_slice(b".text\0\0\0");
        image[0x60..0x64].copy_from_slice(&0x20_u32.to_le_bytes());
        image[0x64..0x68].copy_from_slice(&0x100_u32.to_le_bytes());
        image[0x68..0x6c].copy_from_slice(&0x40_u32.to_le_bytes());

        let base = image.as_ptr() as usize;
        assert_eq!(unsafe { pe_text_section(base) }, Some((base + 0x100, 0x40)));

        image[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(unsafe { pe_text_section(base) }, None);
    }

    #[test]
    fn readable_prefix_stops_before_an_inaccessible_page() {
        const PAGE_SIZE: usize = 4096;
        let allocation = unsafe {
            VirtualAlloc(
                ptr::null(),
                PAGE_SIZE * 2,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        } as *mut u8;
        assert!(!allocation.is_null());

        let mut old_protection = 0;
        assert_ne!(
            unsafe {
                VirtualProtect(
                    allocation.add(PAGE_SIZE).cast(),
                    PAGE_SIZE,
                    PAGE_NOACCESS,
                    &mut old_protection,
                )
            },
            0
        );

        let crossing = unsafe { allocation.add(PAGE_SIZE - 16) };
        assert_eq!(unsafe { readable_prefix(crossing, 32) }, 16);
        assert!(unsafe { readable(crossing, 16) });
        assert!(!unsafe { readable(crossing, 17) });
        assert_ne!(unsafe { VirtualFree(allocation.cast(), 0, MEM_RELEASE) }, 0);
    }

    #[test]
    fn client_schema_matches_static_quest_packet_size() {
        let scalar = 4 | (4 << 8);
        let string8 = 7 | (8 << 8);
        let mut schema = QuestPacketSchemaRecord {
            header: 0x49,
            field_count: 10,
            fields: [0; MAX_QUEST_SCHEMA_FIELDS],
        };
        schema.fields[..10].copy_from_slice(&[
            scalar, scalar, 2, scalar, scalar, scalar, string8, string8, string8, scalar,
        ]);
        assert_eq!(fixed_schema_packet_size(&schema), Some(0x50));
        assert_eq!(quest_packet_size(schema.header), 0x50);
        assert!(QUEST_PACKET_HEADERS.iter().all(
            |header| (1..=MAX_QUEST_PACKET_BYTES).contains(&quest_packet_size(*header as u32))
        ));

        schema.field_count = 3;
        schema.fields[..3].copy_from_slice(&[scalar, 12, scalar]);
        assert_eq!(fixed_schema_packet_size(&schema), None);
        let mut captured = [0_u8; 8];
        captured[..4].copy_from_slice(&0x54_u32.to_le_bytes());
        captured[4..].copy_from_slice(&0x389_u32.to_le_bytes());
        assert!(valid_quest_capture_packet(0x54, &captured));
        captured[4..].copy_from_slice(&0x532c_c66c_u32.to_le_bytes());
        assert!(!valid_quest_capture_packet(0x54, &captured));
    }
}
