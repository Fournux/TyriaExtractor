use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
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
const GAME_SMSG_ITEM_GENERAL_INFO: usize = 0x0161;
const GAME_SMSG_ITEM_REUSE_ID: usize = 0x0162;
const QUEST_PACKET_HEADERS: [usize; 13] = [
    0x0020, 0x0021, 0x0049, 0x004C, 0x0050, 0x0051, 0x0052, 0x0053, 0x0054, 0x0056, 0x007E, 0x0081,
    0x0199,
];
const MAX_QUEST_PACKET_BYTES: usize = 1024;
const MAX_QUEST_SCHEMA_FIELDS: usize = 64;
const QUEST_RING_CAP: usize = 4096;
const MAX_PACKET_BYTES: usize = 768;
const RING_CAP: usize = 4096;
const WRITE_BACKLOG_CAP: usize = 8192;
const CAPTURE_HEALTH_INTERVAL_MS: u128 = 5_000;
const MAX_CAPTURED_ITEM_STRING_U16: usize = 512;
const MAX_CAPTURED_DECODE_IDS: usize = 64;
const TEXT_RESOURCE_DESC_BYTES: usize = 36;
const MAX_TEXT_TRACE_RECORD_BYTES: usize = 1024;
const MAX_TEXT_TRACE_U16: usize = 512;
type TextRecordDecodeFn =
    unsafe extern "system" fn(*mut c_void, *const u8, *const u16, *const u16) -> *const u16;

static RING: LazyLock<Mutex<VecDeque<PacketRecord>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(RING_CAP)));
static QUEST_PACKET_RING: LazyLock<Mutex<VecDeque<QuestPacketRecord>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(QUEST_RING_CAP)));
static OUTPUT_PATH: OnceLock<PathBuf> = OnceLock::new();
static DECODE_ID_RING: LazyLock<Mutex<VecDeque<DecodeIdRecord>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(RING_CAP)));
static TEXT_RESOURCE_REF_RING: LazyLock<Mutex<VecDeque<TextResourceRefRecord>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(RING_CAP)));
static TEXT_TRACE_RING: LazyLock<Mutex<VecDeque<TextDecodeTraceRecord>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(RING_CAP)));
static LAST_TEXT_RESOURCE_REF: LazyLock<Mutex<Option<TextResourceRefSnapshot>>> =
    LazyLock::new(|| Mutex::new(None));
static CAPTURE_SESSION_ID: OnceLock<u64> = OnceLock::new();
static GENERAL_DROPPED_ON_LOCK: AtomicUsize = AtomicUsize::new(0);
static GENERAL_DROPPED_ON_CAPACITY: AtomicUsize = AtomicUsize::new(0);
static GENERAL_WRITE_FAILURES: AtomicUsize = AtomicUsize::new(0);
static QUEST_DROPPED_ON_LOCK: AtomicUsize = AtomicUsize::new(0);
static QUEST_DROPPED_ON_CAPACITY: AtomicUsize = AtomicUsize::new(0);
static QUEST_WRITE_FAILURES: AtomicUsize = AtomicUsize::new(0);

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

fn push_quest_packet(record: QuestPacketRecord) {
    try_push_bounded(
        &QUEST_PACKET_RING,
        QUEST_RING_CAP,
        record,
        &QUEST_DROPPED_ON_LOCK,
        &QUEST_DROPPED_ON_CAPACITY,
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
struct QuestPacketRecord {
    ts_ms: u128,
    session_id: u64,
    header: u32,
    len: u16,
    data: [u8; MAX_QUEST_PACKET_BYTES],
}

#[derive(Clone, Copy)]
struct QuestPacketSchemaRecord {
    header: u32,
    field_count: u16,
    fields: [u32; MAX_QUEST_SCHEMA_FIELDS],
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
static ORIGINAL_QUEST_HANDLERS: [AtomicUsize; QUEST_PACKET_HEADERS.len()] =
    [const { AtomicUsize::new(0) }; QUEST_PACKET_HEADERS.len()];
static ORIGINAL_TEXT_RECORD_DECODE: AtomicUsize = AtomicUsize::new(0);

mod hooks;
mod output;

use hooks::{
    client_pe_timestamp, install_quest_handler_hooks, install_stoc_handler_hooks,
    install_text_decode_id_hook, install_text_record_decode_hook, install_text_resource_ref_hook,
};
use output::{
    append_compact_records, append_decode_id_records, append_quest_packet_records,
    append_quest_schema_records, append_records, append_text_resource_ref_records,
    append_text_trace_records, decode_id_key, drain_decode_id_records, drain_quest_packet_records,
    drain_records, drain_text_resource_ref_records, drain_text_trace_records, write_capture_health,
    write_quest_status, write_status,
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
const PACKETS_JSONL_FILENAME: &str = "tyria_packets.jsonl";
const QUESTS_JSONL_FILENAME: &str = "tyria_quests.jsonl";

static VERBOSE_JSONL: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os(VERBOSE_JSONL_ENV).is_some());

fn verbose_jsonl() -> bool {
    *VERBOSE_JSONL
}

fn output_filename() -> &'static str {
    output_filename_for(verbose_jsonl())
}

fn output_filename_for(verbose: bool) -> &'static str {
    if verbose {
        PACKETS_JSONL_FILENAME
    } else {
        ITEMS_JSONL_FILENAME
    }
}

unsafe extern "system" fn sniffer_thread(param: *mut c_void) -> u32 {
    init_output_path(param as HMODULE);
    let _ = CAPTURE_SESSION_ID.set(now_ms() as u64);
    let verbose = verbose_jsonl();
    let item_hooks_installed = match install_stoc_handler_hooks() {
        Ok(addr) => {
            if verbose {
                let _ = write_status("hook_installed_stoc_handler_table", Some(addr));
            }
            true
        }
        Err(err) => {
            if verbose {
                let _ = write_status(err, None);
            }
            false
        }
    };
    let quest_hooks_installed = match install_quest_handler_hooks() {
        Ok((addr, schemas)) => {
            let timestamp = client_pe_timestamp();
            let _ = append_quest_schema_records(&schemas, capture_session_id(), timestamp);
            let _ = write_quest_status("quest_hooks_installed", Some(addr));
            true
        }
        Err(err) => {
            let _ = write_quest_status(err, None);
            false
        }
    };
    if verbose {
        for install in [
            install_text_decode_id_hook,
            install_text_resource_ref_hook,
            install_text_record_decode_hook,
        ] {
            match install() {
                Ok(addr) => {
                    let _ = write_status("hook_installed_text_decoder", Some(addr));
                }
                Err(err) => {
                    let _ = write_status(err, None);
                }
            }
        }
    }
    if item_hooks_installed || quest_hooks_installed {
        writer_loop();
    }
    0
}

unsafe fn init_output_path(module: HMODULE) {
    let mut buf = [0u16; 1024];
    let len = GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) as usize;
    let mut path = if len == 0 {
        PathBuf::from(output_filename())
    } else {
        let os = OsString::from_wide(&buf[..len]);
        let mut path = PathBuf::from(os);
        path.set_file_name(output_filename());
        path
    };
    if path.as_os_str().is_empty() {
        path = PathBuf::from(output_filename());
    }
    let _ = OUTPUT_PATH.set(path);
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

fn writer_loop() {
    let verbose = verbose_jsonl();
    let mut decode_ids_by_encoded = BTreeMap::<Vec<u8>, DecodeIdRecord>::new();
    let mut quest_write_backlog = Vec::new();
    let mut packet_write_backlog = Vec::new();
    let mut decode_id_write_backlog = Vec::new();
    let mut resource_ref_write_backlog = Vec::new();
    let mut text_trace_write_backlog = Vec::new();
    let mut last_capture_health = 0;
    loop {
        append_retained(
            &mut quest_write_backlog,
            drain_quest_packet_records(),
            &QUEST_DROPPED_ON_CAPACITY,
            append_quest_packet_records,
        );
        let decode_id_records = drain_decode_id_records();
        if verbose {
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
        }

        let records = drain_records();
        if verbose {
            append_retained(
                &mut packet_write_backlog,
                records,
                &GENERAL_DROPPED_ON_CAPACITY,
                |records| append_records(records, &decode_ids_by_encoded),
            );
        } else {
            append_retained(
                &mut packet_write_backlog,
                records,
                &GENERAL_DROPPED_ON_CAPACITY,
                append_compact_records,
            );
        }

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
        if now.saturating_sub(last_capture_health) >= CAPTURE_HEALTH_INTERVAL_MS {
            let _ = write_capture_health();
            last_capture_health = now;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn output_path() -> &'static Path {
    OUTPUT_PATH
        .get_or_init(|| PathBuf::from(output_filename()))
        .as_path()
}

fn quest_output_path() -> PathBuf {
    output_path().with_file_name(QUESTS_JSONL_FILENAME)
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
}
