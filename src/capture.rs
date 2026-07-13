use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

pub(crate) const CAPTURE_MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaptureDomain {
    General,
    Quest,
}

impl CaptureDomain {
    fn label(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Quest => "quest",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct QuestPacketSpec {
    pub(crate) header: u32,
    pub(crate) name: &'static str,
    pub(crate) size: usize,
}

pub(crate) const QUEST_PACKET_SPECS: [QuestPacketSpec; 13] = [
    QuestPacketSpec {
        header: 0x0020,
        name: "AGENT_SPAWNED",
        size: 0x74,
    },
    QuestPacketSpec {
        header: 0x0021,
        name: "AGENT_DESPAWNED",
        size: 8,
    },
    QuestPacketSpec {
        header: 0x0049,
        name: "QUEST_ADD",
        size: 0x50,
    },
    QuestPacketSpec {
        header: 0x004c,
        name: "QUEST_DESCRIPTION",
        size: 0x208,
    },
    QuestPacketSpec {
        header: 0x0050,
        name: "QUEST_GENERAL_INFO",
        size: 0x40,
    },
    QuestPacketSpec {
        header: 0x0051,
        name: "QUEST_UPDATE_MARKER",
        size: 0x18,
    },
    QuestPacketSpec {
        header: 0x0052,
        name: "QUEST_REMOVE",
        size: 8,
    },
    QuestPacketSpec {
        header: 0x0053,
        name: "QUEST_ADD_MARKER",
        size: 0x18,
    },
    QuestPacketSpec {
        header: 0x0054,
        name: "QUEST_UPDATE_OBJECTIVES",
        size: 0x108,
    },
    QuestPacketSpec {
        header: 0x0056,
        name: "NPC_UPDATE_PROPERTIES",
        size: 0x34,
    },
    QuestPacketSpec {
        header: 0x007e,
        name: "DIALOG_BUTTON",
        size: 0x110,
    },
    QuestPacketSpec {
        header: 0x0081,
        name: "DIALOG_SENDER",
        size: 8,
    },
    QuestPacketSpec {
        header: 0x0199,
        name: "INSTANCE_LOAD_INFO",
        size: 0x1c,
    },
];

pub(crate) fn quest_packet_spec(header: u32) -> Option<&'static QuestPacketSpec> {
    QUEST_PACKET_SPECS.iter().find(|spec| spec.header == header)
}

#[derive(Debug, Default, Deserialize)]
struct CaptureHealthRow {
    #[serde(default)]
    session_id: u64,
    #[serde(default)]
    general_dropped_on_lock: u64,
    #[serde(default)]
    general_dropped_on_capacity: u64,
    #[serde(default)]
    general_write_failures: u64,
    #[serde(default)]
    quest_dropped_on_lock: u64,
    #[serde(default)]
    quest_dropped_on_capacity: u64,
    #[serde(default)]
    quest_write_failures: u64,
}

#[derive(Debug, Deserialize)]
struct QuestSchemaRow {
    #[serde(default)]
    session_id: u64,
    header: u32,
    name: String,
    field_count: usize,
    fields: Vec<u32>,
    #[serde(default)]
    client_pe_timestamp: Option<u32>,
}

#[derive(Debug, Default)]
struct SessionCaptureState {
    has_data: bool,
    health_rows: usize,
    dropped_on_lock: u64,
    dropped_on_capacity: u64,
    write_failures: u64,
    schemas: BTreeMap<u32, QuestSchemaRow>,
    client_pe_timestamps: BTreeSet<u32>,
    issues: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CaptureSessionReport {
    session_id: u64,
    health_rows: usize,
    dropped_on_lock: u64,
    dropped_on_capacity: u64,
    write_failures: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    quest_schema_headers: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_pe_timestamp: Option<u32>,
    verified: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    issues: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CaptureIntegrityReport {
    domain: &'static str,
    verified: bool,
    sessions: Vec<CaptureSessionReport>,
}

impl CaptureIntegrityReport {
    pub(crate) fn ensure_verified(&self, path: &Path) -> anyhow::Result<()> {
        if self.verified {
            return Ok(());
        }
        let issues = self
            .sessions
            .iter()
            .flat_map(|session| session.issues.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        bail!(
            "{} capture {} is not verified: {issues}",
            self.domain,
            path.display()
        )
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct CaptureManifest {
    schema_version: u32,
    captures: BTreeMap<String, CaptureIntegrityReport>,
}

impl CaptureManifest {
    pub(crate) fn new(captures: BTreeMap<String, CaptureIntegrityReport>) -> Self {
        Self {
            schema_version: CAPTURE_MANIFEST_SCHEMA_VERSION,
            captures,
        }
    }
}

pub(crate) fn for_each_jsonl_value(
    path: &Path,
    mut visit: impl FnMut(usize, Value) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = line_index + 1;
        let line =
            line.with_context(|| format!("reading {} line {line_number}", path.display()))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value = serde_json::from_str(line)
            .with_context(|| format!("parsing {} line {line_number}", path.display()))?;
        visit(line_number, value)
            .with_context(|| format!("processing {} line {line_number}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn analyze_capture(
    path: &Path,
    domain: CaptureDomain,
) -> anyhow::Result<CaptureIntegrityReport> {
    let mut sessions = BTreeMap::<u64, SessionCaptureState>::new();
    for_each_jsonl_value(path, |line_number, value| {
        let kind = value.get("kind").and_then(Value::as_str);
        let session_id = value
            .get("session_id")
            .and_then(Value::as_u64)
            .unwrap_or_default();

        if kind == Some("capture_health") {
            let health: CaptureHealthRow = serde_json::from_value(value)
                .with_context(|| format!("decoding capture_health at line {line_number}"))?;
            let state = sessions.entry(health.session_id).or_default();
            state.health_rows += 1;
            let (dropped_on_lock, dropped_on_capacity, write_failures) = match domain {
                CaptureDomain::General => (
                    health.general_dropped_on_lock,
                    health.general_dropped_on_capacity,
                    health.general_write_failures,
                ),
                CaptureDomain::Quest => (
                    health.quest_dropped_on_lock,
                    health.quest_dropped_on_capacity,
                    health.quest_write_failures,
                ),
            };
            state.dropped_on_lock = state.dropped_on_lock.max(dropped_on_lock);
            state.dropped_on_capacity = state.dropped_on_capacity.max(dropped_on_capacity);
            state.write_failures = state.write_failures.max(write_failures);
            return Ok(());
        }

        if kind == Some("quest_schema") {
            if domain != CaptureDomain::Quest {
                return Ok(());
            }
            let schema: QuestSchemaRow = serde_json::from_value(value)
                .with_context(|| format!("decoding quest_schema at line {line_number}"))?;
            let state = sessions.entry(schema.session_id).or_default();
            if let Some(timestamp) = schema.client_pe_timestamp {
                state.client_pe_timestamps.insert(timestamp);
            }
            validate_quest_schema(&schema, &mut state.issues);
            match state.schemas.entry(schema.header) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(schema);
                }
                std::collections::btree_map::Entry::Occupied(entry) => {
                    let current = entry.get();
                    if current.name != schema.name
                        || current.field_count != schema.field_count
                        || current.fields != schema.fields
                    {
                        state.issues.push(format!(
                            "session {} has conflicting schema rows for header 0x{:04X}",
                            schema.session_id, schema.header
                        ));
                    }
                }
            }
            return Ok(());
        }

        let has_data = match domain {
            CaptureDomain::Quest => matches!(kind, Some("quest_packet" | "quest_snapshot")),
            CaptureDomain::General => {
                value.get("model_id").is_some()
                    || value
                        .get("decoded")
                        .and_then(|decoded| decoded.get("model_id"))
                        .is_some()
            }
        };
        if has_data {
            sessions.entry(session_id).or_default().has_data = true;
        }
        Ok(())
    })?;

    if !sessions.values().any(|state| state.has_data) {
        let session_id = sessions.keys().next().copied().unwrap_or_default();
        let state = sessions.entry(session_id).or_default();
        state
            .issues
            .push(format!("{} capture contains no data rows", domain.label()));
    }

    let mut reports = Vec::with_capacity(sessions.len());
    for (session_id, mut state) in sessions {
        if state.has_data && state.health_rows == 0 {
            state.issues.push(format!(
                "session {session_id} has data but no capture_health row"
            ));
        }
        if state.dropped_on_lock != 0 {
            state.issues.push(format!(
                "session {session_id} dropped {} records on lock contention",
                state.dropped_on_lock
            ));
        }
        if state.dropped_on_capacity != 0 {
            state.issues.push(format!(
                "session {session_id} dropped {} records at queue capacity",
                state.dropped_on_capacity
            ));
        }
        if state.write_failures != 0 {
            state.issues.push(format!(
                "session {session_id} recorded {} write failures",
                state.write_failures
            ));
        }
        if domain == CaptureDomain::Quest && state.has_data {
            for spec in QUEST_PACKET_SPECS {
                if !state.schemas.contains_key(&spec.header) {
                    state.issues.push(format!(
                        "session {session_id} is missing quest schema 0x{:04X} {}",
                        spec.header, spec.name
                    ));
                }
            }
            if state.client_pe_timestamps.len() > 1 {
                state.issues.push(format!(
                    "session {session_id} has conflicting client PE timestamps"
                ));
            }
        }
        let verified = state.issues.is_empty();
        reports.push(CaptureSessionReport {
            session_id,
            health_rows: state.health_rows,
            dropped_on_lock: state.dropped_on_lock,
            dropped_on_capacity: state.dropped_on_capacity,
            write_failures: state.write_failures,
            quest_schema_headers: state.schemas.into_keys().collect(),
            client_pe_timestamp: state.client_pe_timestamps.first().copied(),
            verified,
            issues: state.issues,
        });
    }
    Ok(CaptureIntegrityReport {
        domain: domain.label(),
        verified: reports.iter().all(|session| session.verified),
        sessions: reports,
    })
}

fn validate_quest_schema(schema: &QuestSchemaRow, issues: &mut Vec<String>) {
    let Some(spec) = quest_packet_spec(schema.header) else {
        issues.push(format!(
            "session {} contains unknown quest schema header 0x{:04X}",
            schema.session_id, schema.header
        ));
        return;
    };
    if schema.name != spec.name {
        issues.push(format!(
            "session {} schema 0x{:04X} is named {} instead of {}",
            schema.session_id, schema.header, schema.name, spec.name
        ));
    }
    if schema.field_count != schema.fields.len() {
        issues.push(format!(
            "session {} schema 0x{:04X} field_count {} does not match {} fields",
            schema.session_id,
            schema.header,
            schema.field_count,
            schema.fields.len()
        ));
        return;
    }
    if fixed_schema_packet_size(&schema.fields) != Some(spec.size) {
        issues.push(format!(
            "session {} schema 0x{:04X} no longer describes a {} byte packet",
            schema.session_id, schema.header, spec.size
        ));
    }
}

fn fixed_schema_packet_size(fields: &[u32]) -> Option<usize> {
    if fields.is_empty() {
        return None;
    }
    fields[1..].iter().try_fold(4_usize, |size, descriptor| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_log(name: &str) -> std::path::PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "tyria-capture-{name}-{}-{id}.jsonl",
            std::process::id()
        ))
    }

    fn schema_row(spec: QuestPacketSpec) -> Value {
        let utf16_count = (spec.size - 4) / 2;
        serde_json::json!({
            "kind": "quest_schema",
            "session_id": 7,
            "header": spec.header,
            "name": spec.name,
            "field_count": 2,
            "fields": [4, ((utf16_count as u32) << 8) | 7],
            "client_pe_timestamp": 1234
        })
    }

    #[test]
    fn verifies_complete_lossless_quest_capture() -> anyhow::Result<()> {
        let path = temp_log("verified");
        let mut rows = QUEST_PACKET_SPECS
            .into_iter()
            .map(schema_row)
            .collect::<Vec<_>>();
        rows.push(serde_json::json!({
            "kind": "quest_packet",
            "session_id": 7,
            "header": 0x49,
            "raw_hex": ""
        }));
        rows.push(serde_json::json!({
            "kind": "capture_health",
            "session_id": 7,
            "quest_dropped_on_lock": 0,
            "quest_dropped_on_capacity": 0,
            "quest_write_failures": 0
        }));
        fs::write(
            &path,
            rows.into_iter()
                .map(|row| row.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )?;

        let report = analyze_capture(&path, CaptureDomain::Quest)?;
        report.ensure_verified(&path)?;
        let manifest = CaptureManifest::new(BTreeMap::from([("quests".to_string(), report)]));
        let first = serde_json::to_vec_pretty(&manifest)?;
        let second = serde_json::to_vec_pretty(&manifest)?;
        assert_eq!(first, second);
        assert!(!String::from_utf8(first)?.contains("generated_at"));
        fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn rejects_capture_losses_and_missing_schema() -> anyhow::Result<()> {
        let path = temp_log("lossy");
        fs::write(
            &path,
            concat!(
                "{\"kind\":\"quest_packet\",\"session_id\":9,\"header\":73,\"raw_hex\":\"\"}\n",
                "{\"kind\":\"capture_health\",\"session_id\":9,\"quest_dropped_on_capacity\":2}"
            ),
        )?;

        let report = analyze_capture(&path, CaptureDomain::Quest)?;
        let error = report
            .ensure_verified(&path)
            .expect_err("lossy capture must fail");
        let message = format!("{error:#}");
        assert!(message.contains("dropped 2 records"));
        assert!(message.contains("missing quest schema"));
        fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn rejects_general_capture_write_failures() -> anyhow::Result<()> {
        let path = temp_log("general-loss");
        fs::write(
            &path,
            concat!(
                "{\"kind\":\"decoded_item\",\"session_id\":11,\"model_id\":32,\"model_file_id\":222}\n",
                "{\"kind\":\"capture_health\",\"session_id\":11,\"general_write_failures\":1}"
            ),
        )?;

        let report = analyze_capture(&path, CaptureDomain::General)?;
        let error = report
            .ensure_verified(&path)
            .expect_err("general capture write failure must fail");
        assert!(format!("{error:#}").contains("recorded 1 write failures"));
        fs::remove_file(path)?;
        Ok(())
    }
}
