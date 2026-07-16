use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::{OsStr, OsString};
use std::os::raw::c_void;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock, TryLockError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::packet;

use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HINSTANCE, HMODULE, TRUE};
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::LibraryLoader::{
    DisableThreadLibraryCalls, GetModuleFileNameW, GetModuleHandleA,
};
use windows_sys::Win32::System::Memory::{
    VirtualAlloc, VirtualProtect, VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_RESERVE,
    PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS,
    PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY,
};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::{CreateThread, GetCurrentProcess};

const STOC_HANDLER_ARRAY_PATTERN: &[u8] = b"\x75\x04\x33\xC0\x5D\xC3\x8B\x41\x08\xA8\x01\x75";
const TEXT_DECODE_IDS_HOOK_PATTERN: &[u8] =
    b"\x8B\x3F\x8B\x83\x68\x01\x00\x00\x8D\x04\x87\x89\x45\x0C\x3B\xF8\x74\x19\xFF\x46\x30";
const TEXT_DECODE_IDS_HOOK_LEN: usize = 8;
const TEXT_RESOURCE_REF_HOOK_PATTERN: &[u8] =
    b"\x8B\x43\x34\x8D\x0C\xF6\x8D\x1C\x88\x85\xDB\x75\x14";
const TEXT_RESOURCE_REF_HOOK_LEN: usize = 6;
const TEXT_RECORD_DECODE_PATTERN: &[u8] =
    b"\x55\x8B\xEC\x83\xEC\x10\x53\x56\x57\x8B\x7D\x0C\x66\x83\x3F\x06";
const TEXT_RECORD_DECODE_HOOK_LEN: usize = 9;
const COLLECTOR_CREATE_ANCHOR: &[u8] = b"\x8B\x1B\x89\x5D\xE8\x83\x3B\x02\x74\x14";
const COLLECTOR_CREATE_ANCHOR_OFFSET: usize = 0x34;
const COLLECTOR_CREATE_HOOK_LEN: usize = 6;
const MERCHANT_CREATE_ANCHOR: &[u8] = b"\x8B\x36\x89\x75\xFC\x83\x3E\x01\x74\x14";
const CRAFTER_CREATE_ANCHOR: &[u8] = b"\x8B\x36\x89\x75\xFC\x83\x3E\x03\x74\x14";
const TRAINER_CREATE_ANCHOR: &[u8] = b"\x8B\x3E\x83\x3F\x0A\x74\x14";
const VENDOR_CREATE_ANCHOR_OFFSET: usize = 0x2A;
const VENDOR_CREATE_HOOK_LEN: usize = 6;
const MANAGER_FIND_AGENT_ANCHOR: &[u8] = b"\x72\x04\x33\xC0\x5D\xC3\xA1";
const MANAGER_FIND_AGENT_ANCHOR_OFFSET: usize = 12;
const ITEM_DATA_BY_ID_ANCHOR: &[u8] = b"\x8B\x75\x08\x8B\x40\x40\x3B\xB0\xC0\x00\x00\x00\x73\x13";
const ITEM_DATA_BY_ID_ANCHOR_OFFSET: usize = 9;
const QUEST_INFO_ASSERTION: &[u8] = b"context->challengeSortArray.Find(challenge)";
const GAME_SMSG_ITEM_GENERAL_INFO: usize = 0x0161;
const GAME_SMSG_ITEM_REUSE_ID: usize = 0x0162;
const WORLD_PACKET_HEADERS: [usize; 16] = [
    0x0020, 0x0021, 0x0049, 0x004C, 0x0050, 0x0051, 0x0052, 0x0053, 0x0054, 0x0056, 0x007E, 0x0081,
    0x009B, 0x00C3, 0x00C4, 0x0199,
];
const MAX_WORLD_PACKET_BYTES: usize = 1024;
const MAX_WORLD_SCHEMA_FIELDS: usize = 64;
const WORLD_RING_CAP: usize = 4096;
const MAX_COLLECTOR_REWARDS: usize = 128;
const COLLECTOR_RING_CAP: usize = 256;
const MAX_VENDOR_ENTRIES: usize = 512;
const VENDOR_RING_CAP: usize = 128;
const MAX_PACKET_BYTES: usize = 768;
const RING_CAP: usize = 4096;
const WRITE_BACKLOG_CAP: usize = 8192;
const CAPTURE_HEALTH_INTERVAL_MS: u128 = 5_000;
const CAPTURE_FORMAT_VERSION: u32 = 5;
const MAX_CAPTURED_ITEM_STRING_U16: usize = 512;
const MAX_CAPTURED_DECODE_IDS: usize = 64;
const TEXT_RESOURCE_DESC_BYTES: usize = 36;
const MAX_TEXT_TRACE_RECORD_BYTES: usize = 1024;
const MAX_TEXT_TRACE_U16: usize = 512;
type TextRecordDecodeFn =
    unsafe extern "system" fn(*mut c_void, *const u8, *const u16, *const u16) -> *const u16;

static RING: LazyLock<Mutex<VecDeque<PacketRecord>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(RING_CAP)));
static RUNTIME_ITEM_STRING_RING: LazyLock<Mutex<VecDeque<RuntimeItemStringsRecord>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(RING_CAP)));
static WORLD_PACKET_RING: LazyLock<Mutex<VecDeque<WorldPacketRecord>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(WORLD_RING_CAP)));
static COLLECTOR_OFFER_RING: LazyLock<Mutex<VecDeque<CollectorOfferRecord>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(COLLECTOR_RING_CAP)));
static VENDOR_CATALOG_RING: LazyLock<Mutex<VecDeque<VendorCatalogRecord>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(VENDOR_RING_CAP)));
static OUTPUT_PATH: OnceLock<PathBuf> = OnceLock::new();
static DECODE_ID_RING: LazyLock<Mutex<VecDeque<DecodeIdRecord>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(RING_CAP)));
static TEXT_RESOURCE_REF_RING: LazyLock<Mutex<VecDeque<TextResourceRefRecord>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(RING_CAP)));
static TEXT_TRACE_RING: LazyLock<Mutex<VecDeque<TextDecodeTraceRecord>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(RING_CAP)));
static LAST_TEXT_RESOURCE_REF: LazyLock<Mutex<Option<TextResourceRefSnapshot>>> =
    LazyLock::new(|| Mutex::new(None));
static REQUESTED_QUEST_IDS: LazyLock<Mutex<BTreeSet<u32>>> =
    LazyLock::new(|| Mutex::new(BTreeSet::new()));
static CAPTURE_SESSION_ID: OnceLock<u64> = OnceLock::new();
static GENERAL_DROPPED_ON_LOCK: AtomicUsize = AtomicUsize::new(0);
static GENERAL_DROPPED_ON_CAPACITY: AtomicUsize = AtomicUsize::new(0);
static GENERAL_WRITE_FAILURES: AtomicUsize = AtomicUsize::new(0);
static WORLD_DROPPED_ON_LOCK: AtomicUsize = AtomicUsize::new(0);
static WORLD_DROPPED_ON_CAPACITY: AtomicUsize = AtomicUsize::new(0);
static WORLD_WRITE_FAILURES: AtomicUsize = AtomicUsize::new(0);

static REQUEST_QUEST_INFO: AtomicUsize = AtomicUsize::new(0);
static CAPTURE_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, PartialEq, Eq)]
enum CaptureStream {
    Quests,
    Npcs,
    VendorContext,
}

fn capture_stream_for_header(header: u32) -> CaptureStream {
    if matches!(header, 0x0020 | 0x0021 | 0x0056 | 0x009B | 0x0199) {
        CaptureStream::Npcs
    } else if matches!(header, 0x00C3 | 0x00C4) {
        CaptureStream::VendorContext
    } else {
        CaptureStream::Quests
    }
}
fn try_push_bounded<T>(
    ring: &Mutex<VecDeque<T>>,
    capacity: usize,
    record: T,
    dropped_on_lock: &AtomicUsize,
    dropped_on_capacity: &AtomicUsize,
) {
    let mut ring = match ring.try_lock() {
        Ok(ring) => ring,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => {
            dropped_on_lock.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };
    if ring.len() >= capacity {
        ring.pop_front();
        dropped_on_capacity.fetch_add(1, Ordering::Relaxed);
    }
    ring.push_back(record);
}

fn push_bounded<T>(ring: &Mutex<VecDeque<T>>, record: T) {
    try_push_bounded(
        ring,
        RING_CAP,
        record,
        &GENERAL_DROPPED_ON_LOCK,
        &GENERAL_DROPPED_ON_CAPACITY,
    );
}

fn push_world_packet(record: WorldPacketRecord) {
    try_push_bounded(
        &WORLD_PACKET_RING,
        WORLD_RING_CAP,
        record,
        &WORLD_DROPPED_ON_LOCK,
        &WORLD_DROPPED_ON_CAPACITY,
    );
}

fn push_collector_offer(record: CollectorOfferRecord) {
    try_push_bounded(
        &COLLECTOR_OFFER_RING,
        COLLECTOR_RING_CAP,
        record,
        &WORLD_DROPPED_ON_LOCK,
        &WORLD_DROPPED_ON_CAPACITY,
    );
}

fn push_vendor_catalog(record: VendorCatalogRecord) {
    try_push_bounded(
        &VENDOR_CATALOG_RING,
        VENDOR_RING_CAP,
        record,
        &WORLD_DROPPED_ON_LOCK,
        &WORLD_DROPPED_ON_CAPACITY,
    );
}

fn drain_queue<T>(queue: &Mutex<VecDeque<T>>, capacity: usize) -> Vec<T> {
    let drained = {
        let mut queue = queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::replace(&mut *queue, VecDeque::with_capacity(capacity))
    };
    drained.into_iter().collect()
}

fn capture_session_id() -> u64 {
    CAPTURE_SESSION_ID.get().copied().unwrap_or_default()
}

fn next_capture_seq() -> usize {
    CAPTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

#[repr(C)]
struct PacketRecord {
    ts_ms: u128,
    session_id: u64,
    header: u32,
    len: u16,
    from_handler: bool,
    dispatch_arg0: usize,
    dispatch_arg2: usize,
    dispatch_arg3: usize,
    data: [u8; MAX_PACKET_BYTES],
}
struct RuntimeItemStringsRecord {
    ts_ms: u128,
    session_id: u64,
    capture_seq: usize,
    item_id: u32,
    model_id: u32,
    model_file_id: u32,
    desc_len: u16,
    desc_complete: bool,
    desc_truncated: bool,
    desc: [u16; MAX_CAPTURED_ITEM_STRING_U16],
    complete_name_len: u16,
    complete_name_complete: bool,
    complete_name_truncated: bool,
    complete_name: [u16; MAX_CAPTURED_ITEM_STRING_U16],
}

impl RuntimeItemStringsRecord {
    fn new(item_id: u32) -> Self {
        Self {
            ts_ms: now_ms(),
            session_id: capture_session_id(),
            capture_seq: next_capture_seq(),
            item_id,
            model_id: 0,
            model_file_id: 0,
            desc_len: 0,
            desc_complete: false,
            desc_truncated: false,
            desc: [0; MAX_CAPTURED_ITEM_STRING_U16],
            complete_name_len: 0,
            complete_name_complete: false,
            complete_name_truncated: false,
            complete_name: [0; MAX_CAPTURED_ITEM_STRING_U16],
        }
    }
}

struct WorldPacketRecord {
    ts_ms: u128,
    session_id: u64,
    capture_seq: usize,
    header: u32,
    len: u16,
    data: [u8; MAX_WORLD_PACKET_BYTES],
}

struct CollectorOfferRecord {
    ts_ms: u128,
    session_id: u64,
    capture_seq: usize,
    merchant_agent_id: u32,
    npc_model_id: u32,
    window_transaction_type: u32,
    transaction_service: u32,
    trophy_item_id: u32,
    required_item_model_id: u32,
    required_item_quantity: u32,
    reward_count: u32,
    captured_reward_count: u16,
    rewards_readable: bool,
    rewards_truncated: bool,
    reward_item_ids: [u32; MAX_COLLECTOR_REWARDS],
    reward_model_ids: [u32; MAX_COLLECTOR_REWARDS],
    reward_model_file_ids: [u32; MAX_COLLECTOR_REWARDS],
    reward_item_types: [u32; MAX_COLLECTOR_REWARDS],
    reward_resolved: [bool; MAX_COLLECTOR_REWARDS],
}

impl Default for CollectorOfferRecord {
    fn default() -> Self {
        Self {
            ts_ms: 0,
            session_id: 0,
            merchant_agent_id: 0,
            capture_seq: 0,
            npc_model_id: 0,
            window_transaction_type: 0,
            transaction_service: 0,
            trophy_item_id: 0,
            required_item_model_id: 0,
            required_item_quantity: 0,
            reward_count: 0,
            captured_reward_count: 0,
            rewards_readable: false,
            rewards_truncated: false,
            reward_item_ids: [0; MAX_COLLECTOR_REWARDS],
            reward_model_ids: [0; MAX_COLLECTOR_REWARDS],
            reward_model_file_ids: [0; MAX_COLLECTOR_REWARDS],
            reward_item_types: [0; MAX_COLLECTOR_REWARDS],
            reward_resolved: [false; MAX_COLLECTOR_REWARDS],
        }
    }
}

#[derive(Clone, Copy, Default)]
struct VendorCatalogEntry {
    source_id: u32,
    aux: u32,
    model_id: u32,
    model_file_id: u32,
    item_type: u32,
    base_value: u32,
    resolved: bool,
}

struct VendorCatalogRecord {
    ts_ms: u128,
    session_id: u64,
    merchant_agent_id: u32,
    capture_seq: usize,
    npc_model_id: u32,
    window_transaction_type: u32,
    transaction_service: u32,
    entry_count: u32,
    captured_entry_count: u16,
    entries_readable: bool,
    entries_truncated: bool,
    entries: [VendorCatalogEntry; MAX_VENDOR_ENTRIES],
}

impl Default for VendorCatalogRecord {
    fn default() -> Self {
        Self {
            ts_ms: 0,
            session_id: 0,
            merchant_agent_id: 0,
            npc_model_id: 0,
            capture_seq: 0,
            window_transaction_type: 0,
            transaction_service: 0,
            entry_count: 0,
            captured_entry_count: 0,
            entries_readable: false,
            entries_truncated: false,
            entries: [VendorCatalogEntry::default(); MAX_VENDOR_ENTRIES],
        }
    }
}

#[derive(Clone, Copy)]
struct WorldPacketSchemaRecord {
    header: u32,
    field_count: u16,
    fields: [u32; MAX_WORLD_SCHEMA_FIELDS],
}

#[derive(Clone)]
struct DecodeIdRecord {
    ts_ms: u128,
    language_id: u32,
    encoded_len: u16,
    encoded: [u16; MAX_CAPTURED_ITEM_STRING_U16],
    id_count: u16,
    ids: [u32; MAX_CAPTURED_DECODE_IDS],
}

impl Default for DecodeIdRecord {
    fn default() -> Self {
        Self {
            ts_ms: 0,
            language_id: 0,
            encoded_len: 0,
            encoded: [0; MAX_CAPTURED_ITEM_STRING_U16],
            id_count: 0,
            ids: [0; MAX_CAPTURED_DECODE_IDS],
        }
    }
}

struct TextResourceRefRecord {
    ts_ms: u128,
    language_id: u32,
    decoded_id: u32,
    text_file_index: u32,
    record_index: u32,
    file_desc: [u8; TEXT_RESOURCE_DESC_BYTES],
}
#[derive(Clone, Copy)]
struct TextResourceRefSnapshot {
    ts_ms: u128,
    language_id: u32,
    decoded_id: u32,
    text_file_index: u32,
    record_index: u32,
}

struct TextDecodeTraceRecord {
    ts_ms: u128,
    has_ref: bool,
    ref_language_id: u32,
    ref_decoded_id: u32,
    ref_text_file_index: u32,
    ref_record_index: u32,
    ref_age_ms: u32,
    language_id: u32,
    context: usize,
    record_ptr: usize,
    output_ptr: usize,
    substitute_start: usize,
    substitute_end: usize,
    record_size: u16,
    compression_or_flags: u16,
    record_type: u8,
    record_subtype: u8,
    record_bytes_len: u16,
    record_truncated: bool,
    record_bytes: [u8; MAX_TEXT_TRACE_RECORD_BYTES],
    output_len: u16,
    output_truncated: bool,
    output: [u16; MAX_TEXT_TRACE_U16],
    substitute_len: u16,
    substitute_truncated: bool,
    substitute: [u16; MAX_TEXT_TRACE_U16],
}

impl TextDecodeTraceRecord {
    fn new() -> Self {
        Self {
            ts_ms: 0,
            has_ref: false,
            ref_language_id: 0,
            ref_decoded_id: 0,
            ref_text_file_index: 0,
            ref_record_index: 0,
            ref_age_ms: 0,
            language_id: 0,
            context: 0,
            record_ptr: 0,
            output_ptr: 0,
            substitute_start: 0,
            substitute_end: 0,
            record_size: 0,
            compression_or_flags: 0,
            record_type: 0,
            record_subtype: 0,
            record_bytes_len: 0,
            record_truncated: false,
            record_bytes: [0; MAX_TEXT_TRACE_RECORD_BYTES],
            output_len: 0,
            output_truncated: false,
            output: [0; MAX_TEXT_TRACE_U16],
            substitute_len: 0,
            substitute_truncated: false,
            substitute: [0; MAX_TEXT_TRACE_U16],
        }
    }
}

type StoCHandlerFn = unsafe extern "C" fn(usize, *mut u8, usize, usize) -> bool;
type VendorCreateFn = unsafe extern "thiscall" fn(*mut u8, *mut *mut u8);
type ManagerFindAgentFn = unsafe extern "C" fn(u32) -> *const u8;
type ItemDataByIdFn = unsafe extern "C" fn(u32) -> *const u8;
type RequestQuestInfoFn = unsafe extern "C" fn(u32);

#[repr(C)]
struct StoCHandler {
    fields: *mut u32,
    field_count: u32,
    handler_func: usize,
}

#[repr(C)]
struct GwArray<T> {
    buffer: *mut T,
    capacity: u32,
    size: u32,
    param: u32,
}

#[repr(C)]
struct GameServer {
    _pad0: [u8; 8],
    gs_codec: *mut GameServerCodec,
}

#[repr(C)]
struct GameServerCodec {
    _pad0: [u8; 12],
    ls_codec: *mut c_void,
    _pad1: [u8; 12],
    client_codec_array: [u32; 4],
    handlers: GwArray<StoCHandler>,
}

static ORIGINAL_ITEM_GENERAL: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_ITEM_REUSE: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_WORLD_HANDLERS: [AtomicUsize; WORLD_PACKET_HEADERS.len()] =
    [const { AtomicUsize::new(0) }; WORLD_PACKET_HEADERS.len()];
static ORIGINAL_TEXT_RECORD_DECODE: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_COLLECTOR_CREATE: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_MERCHANT_CREATE: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_CRAFTER_CREATE: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_TRAINER_CREATE: AtomicUsize = AtomicUsize::new(0);
static MANAGER_FIND_AGENT: AtomicUsize = AtomicUsize::new(0);
static ITEM_DATA_BY_ID: AtomicUsize = AtomicUsize::new(0);
static LAST_VENDOR_AGENT_ID: AtomicUsize = AtomicUsize::new(0);
static LAST_VENDOR_TRANSACTION_TYPE: AtomicUsize = AtomicUsize::new(0);

mod hooks;
mod output;

use hooks::{
    client_pe_timestamp, install_quest_info_request, install_stoc_handler_hooks,
    install_text_decode_id_hook, install_text_record_decode_hook, install_text_resource_ref_hook,
    install_vendor_hooks, install_world_handler_hooks,
};
use output::{
    append_collector_offer_records, append_crafter_catalog_records, append_decode_id_records,
    append_merchant_catalog_records, append_npc_world_packet_records, append_records,
    append_runtime_item_string_records, append_skill_trainer_catalog_records,
    append_text_resource_ref_records, append_text_trace_records,
    append_vendor_context_catalog_records, append_vendor_context_packet_records,
    append_world_packet_records, append_world_packet_schema_records, decode_id_key,
    drain_collector_offer_records, drain_decode_id_records, drain_records,
    drain_runtime_item_string_records, drain_text_resource_ref_records, drain_text_trace_records,
    drain_vendor_catalog_records, drain_world_packet_records, write_capture_health, write_status,
    write_vendor_status, write_world_status,
};

#[no_mangle]
pub unsafe extern "system" fn DllMain(
    hinst: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        DisableThreadLibraryCalls(hinst);
        let handle = CreateThread(
            ptr::null(),
            0,
            Some(sniffer_thread),
            hinst as *const c_void,
            0,
            ptr::null_mut(),
        );
        if !handle.is_null() {
            CloseHandle(handle);
        }
    }
    TRUE
}

const VERBOSE_JSONL_ENV: &str = "TYRIA_VERBOSE_JSONL";
const ITEMS_JSONL_FILENAME: &str = "tyria_items.jsonl";
const QUESTS_JSONL_FILENAME: &str = "tyria_quests.jsonl";
const NPCS_JSONL_FILENAME: &str = "tyria_npcs.jsonl";
const VENDOR_CONTEXT_JSONL_FILENAME: &str = "tyria_vendor_context.jsonl";
const COLLECTORS_JSONL_FILENAME: &str = "tyria_collectors.jsonl";
const MERCHANTS_JSONL_FILENAME: &str = "tyria_merchants.jsonl";
const CRAFTERS_JSONL_FILENAME: &str = "tyria_crafters.jsonl";
const SKILL_TRAINERS_JSONL_FILENAME: &str = "tyria_skill_trainers.jsonl";
const CAPTURE_METADATA_JSONL_FILENAME: &str = "tyria_capture.jsonl";

static VERBOSE_JSONL: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os(VERBOSE_JSONL_ENV).is_some());

fn verbose_jsonl() -> bool {
    *VERBOSE_JSONL
}

fn output_filename() -> &'static str {
    ITEMS_JSONL_FILENAME
}

unsafe extern "system" fn sniffer_thread(param: *mut c_void) -> u32 {
    let session_id = now_ms() as u64;
    let _ = CAPTURE_SESSION_ID.set(session_id);
    init_output_path(param as HMODULE, session_id);
    let verbose = verbose_jsonl();

    let text_decode_hook_installed = match install_text_decode_id_hook() {
        Ok(addr) => {
            let _ = write_status("hook_installed_text_decoder", Some(addr));
            true
        }
        Err(err) => {
            let _ = write_status(err, None);
            false
        }
    };
    let item_hooks_installed = match install_stoc_handler_hooks() {
        Ok(addr) => {
            let _ = write_status("hook_installed_stoc_handler_table", Some(addr));
            true
        }
        Err(err) => {
            let _ = write_status(err, None);
            false
        }
    };
    match install_quest_info_request() {
        Ok(addr) => {
            let _ = write_world_status("quest_info_request_ready", Some(addr));
        }
        Err(err) => {
            let _ = write_world_status(err, None);
        }
    }
    let world_hooks_installed = match install_world_handler_hooks() {
        Ok((addr, schemas)) => {
            let timestamp = client_pe_timestamp();
            let _ = append_world_packet_schema_records(&schemas, capture_session_id(), timestamp);
            let _ = write_world_status("world_hooks_installed", Some(addr));
            true
        }
        Err(err) => {
            let _ = write_world_status(err, None);
            false
        }
    };
    let vendor_hooks_installed = match install_vendor_hooks() {
        Ok(addr) => {
            let _ = write_vendor_status("vendor_hooks_installed", Some(addr));
            true
        }
        Err(err) => {
            let _ = write_world_status(err, None);
            false
        }
    };
    if verbose {
        for install in [
            install_text_resource_ref_hook,
            install_text_record_decode_hook,
        ] {
            match install() {
                Ok(addr) => {
                    let _ = write_status("hook_installed_text_decoder_trace", Some(addr));
                }
                Err(err) => {
                    let _ = write_status(err, None);
                }
            }
        }
    }
    if text_decode_hook_installed
        || item_hooks_installed
        || world_hooks_installed
        || vendor_hooks_installed
    {
        writer_loop();
    }
    0
}

unsafe fn init_output_path(module: HMODULE, session_id: u64) {
    let mut buf = [0u16; 1024];
    let len = GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) as usize;
    let path = if len == 0 {
        PathBuf::from(output_filename())
    } else {
        let module_path = PathBuf::from(OsString::from_wide(&buf[..len]));
        let session_path = session_output_path(&module_path, session_id);
        if session_path
            .parent()
            .is_some_and(|parent| std::fs::create_dir_all(parent).is_ok())
        {
            session_path
        } else {
            module_path.with_file_name(output_filename())
        }
    };
    let _ = OUTPUT_PATH.set(path);
}

fn session_output_path(module_path: &Path, session_id: u64) -> PathBuf {
    let module_dir = module_path.parent().unwrap_or_else(|| Path::new("."));
    let capture_root = module_path
        .ancestors()
        .find(|ancestor| ancestor.file_name() == Some(OsStr::new("target")))
        .and_then(Path::parent)
        .map_or_else(
            || module_dir.join("captures"),
            |repository| repository.join("captures"),
        );
    capture_root
        .join(session_id.to_string())
        .join(output_filename())
}

fn append_retained<T>(
    backlog: &mut Vec<T>,
    mut records: Vec<T>,
    dropped_on_capacity: &AtomicUsize,
    write: impl FnOnce(&[T]) -> std::io::Result<()>,
) {
    backlog.append(&mut records);
    if backlog.len() > WRITE_BACKLOG_CAP {
        let excess = backlog.len() - WRITE_BACKLOG_CAP;
        backlog.drain(..excess);
        dropped_on_capacity.fetch_add(excess, Ordering::Relaxed);
    }
    if !backlog.is_empty() && write(backlog).is_ok() {
        backlog.clear();
    }
}

fn capture_health_counters() -> [usize; 6] {
    [
        GENERAL_DROPPED_ON_LOCK.load(Ordering::Relaxed),
        GENERAL_DROPPED_ON_CAPACITY.load(Ordering::Relaxed),
        GENERAL_WRITE_FAILURES.load(Ordering::Relaxed),
        WORLD_DROPPED_ON_LOCK.load(Ordering::Relaxed),
        WORLD_DROPPED_ON_CAPACITY.load(Ordering::Relaxed),
        WORLD_WRITE_FAILURES.load(Ordering::Relaxed),
    ]
}
fn capture_health_changed(previous: Option<[usize; 6]>, current: [usize; 6]) -> bool {
    previous != Some(current)
}

fn writer_loop() {
    let verbose = verbose_jsonl();
    let mut decode_ids_by_encoded = BTreeMap::<Vec<u8>, DecodeIdRecord>::new();
    let mut quest_write_backlog = Vec::new();
    let mut npc_write_backlog = Vec::new();
    let mut vendor_context_packet_write_backlog = Vec::new();
    let mut collector_write_backlog = Vec::new();
    let mut merchant_write_backlog = Vec::new();
    let mut crafter_write_backlog = Vec::new();
    let mut skill_trainer_write_backlog = Vec::new();
    let mut unknown_vendor_write_backlog = Vec::new();
    let mut packet_write_backlog = Vec::new();
    let mut runtime_item_string_write_backlog = Vec::new();
    let mut decode_id_write_backlog = Vec::new();
    let mut resource_ref_write_backlog = Vec::new();
    let mut text_trace_write_backlog = Vec::new();
    let mut last_capture_health = None;
    let mut last_capture_health_check = 0;
    loop {
        let mut quest_records = Vec::new();
        let mut npc_records = Vec::new();
        let mut vendor_context_packet_records = Vec::new();
        for record in drain_world_packet_records() {
            match capture_stream_for_header(record.header) {
                CaptureStream::Quests => quest_records.push(record),
                CaptureStream::Npcs => npc_records.push(record),
                CaptureStream::VendorContext => vendor_context_packet_records.push(record),
            }
        }
        append_retained(
            &mut quest_write_backlog,
            quest_records,
            &WORLD_DROPPED_ON_CAPACITY,
            append_world_packet_records,
        );
        append_retained(
            &mut npc_write_backlog,
            npc_records,
            &WORLD_DROPPED_ON_CAPACITY,
            append_npc_world_packet_records,
        );
        append_retained(
            &mut vendor_context_packet_write_backlog,
            vendor_context_packet_records,
            &WORLD_DROPPED_ON_CAPACITY,
            append_vendor_context_packet_records,
        );
        append_retained(
            &mut collector_write_backlog,
            drain_collector_offer_records(),
            &WORLD_DROPPED_ON_CAPACITY,
            append_collector_offer_records,
        );

        let mut merchant_records = Vec::new();
        let mut crafter_records = Vec::new();
        let mut skill_trainer_records = Vec::new();
        let mut unknown_vendor_records = Vec::new();
        for record in drain_vendor_catalog_records() {
            match record.transaction_service {
                1 => merchant_records.push(record),
                3 => crafter_records.push(record),
                10 => skill_trainer_records.push(record),
                _ => unknown_vendor_records.push(record),
            }
        }
        append_retained(
            &mut merchant_write_backlog,
            merchant_records,
            &WORLD_DROPPED_ON_CAPACITY,
            append_merchant_catalog_records,
        );
        append_retained(
            &mut crafter_write_backlog,
            crafter_records,
            &WORLD_DROPPED_ON_CAPACITY,
            append_crafter_catalog_records,
        );
        append_retained(
            &mut skill_trainer_write_backlog,
            skill_trainer_records,
            &WORLD_DROPPED_ON_CAPACITY,
            append_skill_trainer_catalog_records,
        );
        append_retained(
            &mut unknown_vendor_write_backlog,
            unknown_vendor_records,
            &WORLD_DROPPED_ON_CAPACITY,
            append_vendor_context_catalog_records,
        );

        let decode_id_records = drain_decode_id_records();
        for record in &decode_id_records {
            decode_ids_by_encoded
                .entry(decode_id_key(record))
                .or_insert_with(|| record.clone());
        }
        append_retained(
            &mut decode_id_write_backlog,
            decode_id_records,
            &GENERAL_DROPPED_ON_CAPACITY,
            append_decode_id_records,
        );
        append_retained(
            &mut packet_write_backlog,
            drain_records(),
            &GENERAL_DROPPED_ON_CAPACITY,
            |records| append_records(records, &decode_ids_by_encoded),
        );
        append_retained(
            &mut runtime_item_string_write_backlog,
            drain_runtime_item_string_records(),
            &GENERAL_DROPPED_ON_CAPACITY,
            append_runtime_item_string_records,
        );

        if verbose {
            append_retained(
                &mut resource_ref_write_backlog,
                drain_text_resource_ref_records(),
                &GENERAL_DROPPED_ON_CAPACITY,
                append_text_resource_ref_records,
            );
            append_retained(
                &mut text_trace_write_backlog,
                drain_text_trace_records(),
                &GENERAL_DROPPED_ON_CAPACITY,
                append_text_trace_records,
            );
        }
        let now = now_ms();
        if now.saturating_sub(last_capture_health_check) >= CAPTURE_HEALTH_INTERVAL_MS {
            let health = capture_health_counters();
            if capture_health_changed(last_capture_health, health) && write_capture_health().is_ok()
            {
                last_capture_health = Some(health);
            }
            last_capture_health_check = now;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn output_path() -> &'static Path {
    OUTPUT_PATH
        .get_or_init(|| PathBuf::from(output_filename()))
        .as_path()
}

fn sibling_output_path(filename: &str) -> PathBuf {
    output_path().with_file_name(filename)
}
fn capture_metadata_output_path() -> PathBuf {
    sibling_output_path(CAPTURE_METADATA_JSONL_FILENAME)
}

fn quest_output_path() -> PathBuf {
    sibling_output_path(QUESTS_JSONL_FILENAME)
}

fn npc_output_path() -> PathBuf {
    sibling_output_path(NPCS_JSONL_FILENAME)
}

fn collector_output_path() -> PathBuf {
    sibling_output_path(COLLECTORS_JSONL_FILENAME)
}

fn vendor_context_output_path() -> PathBuf {
    sibling_output_path(VENDOR_CONTEXT_JSONL_FILENAME)
}

fn merchant_output_path() -> PathBuf {
    sibling_output_path(MERCHANTS_JSONL_FILENAME)
}

fn crafter_output_path() -> PathBuf {
    sibling_output_path(CRAFTERS_JSONL_FILENAME)
}

fn skill_trainer_output_path() -> PathBuf {
    sibling_output_path(SKILL_TRAINERS_JSONL_FILENAME)
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_health_is_written_only_when_counters_change() {
        let zero = [0; 6];
        assert!(capture_health_changed(None, zero));
        assert!(!capture_health_changed(Some(zero), zero));
        assert!(capture_health_changed(Some(zero), [0, 0, 0, 0, 0, 1]));
    }

    #[test]
    fn failed_writer_batches_are_bounded_and_retried() {
        let mut backlog = Vec::new();
        let dropped = AtomicUsize::new(0);
        append_retained(
            &mut backlog,
            (0..WRITE_BACKLOG_CAP + 2).collect(),
            &dropped,
            |records| {
                assert_eq!(records.len(), WRITE_BACKLOG_CAP);
                assert_eq!(records[0], 2);
                Err(std::io::Error::other("simulated write failure"))
            },
        );
        assert_eq!(backlog.len(), WRITE_BACKLOG_CAP);
        assert_eq!(dropped.load(Ordering::Relaxed), 2);

        let mut retried = 0;
        append_retained(&mut backlog, Vec::new(), &dropped, |records| {
            retried = records.len();
            Ok(())
        });
        assert_eq!(retried, WRITE_BACKLOG_CAP);
        assert!(backlog.is_empty());
    }
    #[test]
    fn session_capture_path_lives_outside_target() {
        let module = Path::new(r"C:\repo\target\i686-pc-windows-msvc\release\tyria_sniffer.dll");
        let path = session_output_path(module, 42);
        assert_eq!(path.parent(), Some(Path::new(r"C:\repo\captures\42")));
        assert_eq!(path.file_name(), Some(OsStr::new(output_filename())));
        for filename in [
            QUESTS_JSONL_FILENAME,
            NPCS_JSONL_FILENAME,
            VENDOR_CONTEXT_JSONL_FILENAME,
            COLLECTORS_JSONL_FILENAME,
            MERCHANTS_JSONL_FILENAME,
            CRAFTERS_JSONL_FILENAME,
            SKILL_TRAINERS_JSONL_FILENAME,
        ] {
            assert_eq!(
                path.with_file_name(filename).parent(),
                Some(Path::new(r"C:\repo\captures\42"))
            );
        }
    }

    #[test]
    fn routes_packets_to_dedicated_capture_streams() {
        assert_eq!(capture_stream_for_header(0x0049), CaptureStream::Quests);
        assert_eq!(capture_stream_for_header(0x007E), CaptureStream::Quests);
        assert_eq!(capture_stream_for_header(0x0020), CaptureStream::Npcs);
        assert_eq!(capture_stream_for_header(0x0056), CaptureStream::Npcs);
        assert_eq!(capture_stream_for_header(0x0199), CaptureStream::Npcs);
        assert_eq!(capture_stream_for_header(0x009B), CaptureStream::Npcs);
        assert_eq!(
            capture_stream_for_header(0x00C3),
            CaptureStream::VendorContext
        );
        assert_eq!(
            capture_stream_for_header(0x00C4),
            CaptureStream::VendorContext
        );
    }
}
