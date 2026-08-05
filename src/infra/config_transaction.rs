// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mode-neutral, crash-safe configuration persistence.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::ConfigValidationSummary;
use crate::infra::control::config_version;
use crate::infra::error::{DnsError, Result};

pub const MAX_CONFIG_BYTES: usize = 2 * 1024 * 1024;
const MAX_STATE_BYTES: usize = 16 * 1024 * 1024;
const MAX_HISTORY_BYTES: usize = 20 * 1024 * 1024;
const MAX_HISTORY_FILE_BYTES: usize = 64 * 1024 * 1024;
const MAX_HISTORY_ENTRIES: usize = 20;
const SCHEMA: u8 = 1;

static CONFIG_MUTATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionStatus {
    Pending,
    Succeeded,
    Failed,
    Recovered,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub schema: u8,
    pub transaction_id: String,
    pub status: TransactionStatus,
    pub created_at_ms: u64,
    pub completed_at_ms: Option<u64>,
    pub previous_config_version: String,
    pub candidate_config_version: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransactionJournal {
    schema: u8,
    transaction_id: String,
    status: TransactionStatus,
    created_at_ms: u64,
    previous_config_version: String,
    candidate_config_version: String,
    previous_config: String,
    candidate_config: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub created_at_ms: u64,
    pub config_version: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct HistoryStore {
    schema: u8,
    entries: Vec<HistoryEntry>,
}

#[derive(Debug)]
pub enum BeginError {
    Busy,
    VersionConflict,
    CandidateVersionConflict,
    TooLarge { actual: usize, max: usize },
    Invalid(String),
    Io(String),
}

pub fn mutation_guard() -> std::result::Result<MutexGuard<'static, ()>, String> {
    CONFIG_MUTATION_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "configuration mutation lock is poisoned".to_string())
}

pub fn has_pending(config_path: &Path) -> bool {
    pending_path(config_path).exists()
}

pub fn validate_candidate(
    config_path: &Path,
    content: &str,
) -> std::result::Result<ConfigValidationSummary, String> {
    ensure_content_size(content).map_err(|(actual, max)| {
        format!("configuration is too large: {actual} bytes > {max} bytes")
    })?;
    let temp_path = adjacent_temp_path(config_path, "validate");
    atomic_replace(&temp_path, content.as_bytes(), true)?;
    let result = crate::config::validate_file(&temp_path).map_err(|err| err.to_string());
    let _ = fs::remove_file(temp_path);
    result
}

pub fn begin(
    config_path: &Path,
    healthy_content: &str,
    candidate_content: &str,
    base_version: &str,
    candidate_version: &str,
) -> std::result::Result<TransactionRecord, BeginError> {
    let _guard = mutation_guard().map_err(BeginError::Io)?;
    if has_pending(config_path) {
        return Err(BeginError::Busy);
    }
    if let Err((actual, max)) = ensure_content_size(candidate_content) {
        return Err(BeginError::TooLarge { actual, max });
    }
    let current = fs::read_to_string(config_path).map_err(|err| {
        BeginError::Io(format!(
            "failed to read config {}: {err}",
            config_path.display()
        ))
    })?;
    if config_version(&current) != base_version {
        return Err(BeginError::VersionConflict);
    }
    let computed_candidate = config_version(candidate_content);
    if computed_candidate != candidate_version {
        return Err(BeginError::CandidateVersionConflict);
    }
    validate_candidate(config_path, candidate_content).map_err(BeginError::Invalid)?;

    let created_at_ms = unix_time_ms();
    let transaction_id = format!(
        "config-{created_at_ms}-{}-{}",
        std::process::id(),
        &computed_candidate[..12]
    );
    let journal = TransactionJournal {
        schema: SCHEMA,
        transaction_id: transaction_id.clone(),
        status: TransactionStatus::Pending,
        created_at_ms,
        previous_config_version: config_version(healthy_content),
        candidate_config_version: computed_candidate.clone(),
        previous_config: healthy_content.to_string(),
        candidate_config: candidate_content.to_string(),
    };
    write_json(&pending_path(config_path), &journal, MAX_STATE_BYTES).map_err(BeginError::Io)?;
    if let Err(message) = atomic_replace(config_path, candidate_content.as_bytes(), false) {
        let _ = remove_file_if_exists(&pending_path(config_path));
        return Err(BeginError::Io(message));
    }
    Ok(record_from_journal(
        &journal,
        TransactionStatus::Pending,
        None,
    ))
}

pub fn status(config_path: &Path) -> std::result::Result<Option<TransactionRecord>, String> {
    let _guard = mutation_guard()?;
    if let Some(journal) = read_journal(config_path)? {
        return Ok(Some(record_from_journal(
            &journal,
            TransactionStatus::Pending,
            None,
        )));
    }
    read_json(
        &last_path(config_path),
        "last configuration transaction",
        MAX_STATE_BYTES,
    )
}

pub fn pending_candidate(config_path: &Path) -> std::result::Result<Option<String>, String> {
    let _guard = mutation_guard()?;
    Ok(read_journal(config_path)?.map(|journal| journal.candidate_config))
}

/// Commit the durable transaction record. Healthy-history persistence is
/// deliberately best-effort and only emits a warning.
pub fn finalize(config_path: &Path) -> Result<()> {
    let _guard = mutation_guard().map_err(DnsError::runtime)?;
    let Some(journal) = read_journal(config_path).map_err(DnsError::runtime)? else {
        return Ok(());
    };
    let record = record_from_journal(&journal, TransactionStatus::Succeeded, None);
    write_json(&last_path(config_path), &record, MAX_STATE_BYTES).map_err(DnsError::runtime)?;
    remove_file_if_exists(&pending_path(config_path)).map_err(DnsError::runtime)?;
    if let Err(error) = append_history(config_path, &journal) {
        tracing::warn!(%error, "healthy configuration history update failed");
    }
    Ok(())
}

pub fn rollback(config_path: &Path, error: impl Into<String>) -> Result<bool> {
    let _guard = mutation_guard().map_err(DnsError::runtime)?;
    let Some(journal) = read_journal(config_path).map_err(DnsError::runtime)? else {
        return Ok(false);
    };
    atomic_replace(config_path, journal.previous_config.as_bytes(), false)
        .map_err(DnsError::runtime)?;
    let record = record_from_journal(
        &journal,
        TransactionStatus::Failed,
        Some(bounded_error(error.into())),
    );
    write_json(&last_path(config_path), &record, MAX_STATE_BYTES).map_err(DnsError::runtime)?;
    remove_file_if_exists(&pending_path(config_path)).map_err(DnsError::runtime)?;
    Ok(true)
}

pub fn recover_pending(config_path: &Path) -> Result<bool> {
    let _guard = mutation_guard().map_err(DnsError::runtime)?;
    let Some(journal) = read_journal(config_path).map_err(DnsError::runtime)? else {
        return Ok(false);
    };
    atomic_replace(config_path, journal.previous_config.as_bytes(), false)
        .map_err(DnsError::runtime)?;
    let record = record_from_journal(
        &journal,
        TransactionStatus::Recovered,
        Some("interrupted configuration apply was rolled back during startup".to_string()),
    );
    write_json(&last_path(config_path), &record, MAX_STATE_BYTES).map_err(DnsError::runtime)?;
    remove_file_if_exists(&pending_path(config_path)).map_err(DnsError::runtime)?;
    Ok(true)
}

pub fn history(config_path: &Path) -> std::result::Result<Vec<HistoryEntry>, String> {
    let _guard = mutation_guard()?;
    Ok(read_history(config_path)?.entries)
}

pub fn history_entry(
    config_path: &Path,
    id: &str,
) -> std::result::Result<Option<HistoryEntry>, String> {
    let _guard = mutation_guard()?;
    Ok(read_history(config_path)?
        .entries
        .into_iter()
        .find(|entry| entry.id == id))
}

pub fn record_initial_healthy(
    config_path: &Path,
    content: &str,
) -> std::result::Result<(), String> {
    let _guard = mutation_guard()?;
    ensure_content_size(content).map_err(|(actual, max)| {
        format!("configuration is too large: {actual} bytes > {max} bytes")
    })?;
    let journal = TransactionJournal {
        schema: SCHEMA,
        transaction_id: format!("startup-{}-{}", unix_time_ms(), std::process::id()),
        status: TransactionStatus::Succeeded,
        created_at_ms: unix_time_ms(),
        previous_config_version: config_version(content),
        candidate_config_version: config_version(content),
        previous_config: content.to_string(),
        candidate_config: content.to_string(),
    };
    append_history(config_path, &journal)
}

pub fn atomic_write_config(config_path: &Path, content: &str) -> std::result::Result<(), String> {
    ensure_content_size(content).map_err(|(actual, max)| {
        format!("configuration is too large: {actual} bytes > {max} bytes")
    })?;
    atomic_replace(config_path, content.as_bytes(), false)
}

fn ensure_content_size(content: &str) -> std::result::Result<(), (usize, usize)> {
    if content.len() > MAX_CONFIG_BYTES {
        Err((content.len(), MAX_CONFIG_BYTES))
    } else {
        Ok(())
    }
}

fn append_history(
    config_path: &Path,
    journal: &TransactionJournal,
) -> std::result::Result<(), String> {
    let mut store = read_history(config_path)?;
    store
        .entries
        .retain(|entry| entry.config_version != journal.candidate_config_version);
    store.entries.insert(
        0,
        HistoryEntry {
            id: journal.transaction_id.clone(),
            created_at_ms: unix_time_ms(),
            config_version: journal.candidate_config_version.clone(),
            content: journal.candidate_config.clone(),
        },
    );
    store.entries.truncate(MAX_HISTORY_ENTRIES);
    while history_payload_bytes(&store.entries) > MAX_HISTORY_BYTES {
        if store.entries.pop().is_none() {
            break;
        }
    }
    write_json(&history_path(config_path), &store, MAX_HISTORY_FILE_BYTES)
}

fn history_payload_bytes(entries: &[HistoryEntry]) -> usize {
    entries.iter().map(|entry| entry.content.len()).sum()
}

fn read_history(config_path: &Path) -> std::result::Result<HistoryStore, String> {
    let store = read_json(
        &history_path(config_path),
        "configuration history",
        MAX_HISTORY_FILE_BYTES,
    )?
    .unwrap_or(HistoryStore {
        schema: SCHEMA,
        entries: Vec::new(),
    });
    if store.schema != SCHEMA {
        return Err(format!(
            "unsupported configuration history schema {}",
            store.schema
        ));
    }
    Ok(store)
}

fn read_journal(config_path: &Path) -> std::result::Result<Option<TransactionJournal>, String> {
    let journal: Option<TransactionJournal> = read_json(
        &pending_path(config_path),
        "configuration transaction",
        MAX_STATE_BYTES,
    )?;
    if journal
        .as_ref()
        .is_some_and(|journal| journal.schema != SCHEMA)
    {
        return Err("unsupported configuration transaction schema".to_string());
    }
    Ok(journal)
}

fn record_from_journal(
    journal: &TransactionJournal,
    status: TransactionStatus,
    error: Option<String>,
) -> TransactionRecord {
    TransactionRecord {
        schema: SCHEMA,
        transaction_id: journal.transaction_id.clone(),
        status,
        created_at_ms: journal.created_at_ms,
        completed_at_ms: (status != TransactionStatus::Pending).then(unix_time_ms),
        previous_config_version: journal.previous_config_version.clone(),
        candidate_config_version: journal.candidate_config_version.clone(),
        error,
    }
}

fn bounded_error(mut message: String) -> String {
    const MAX: usize = 4096;
    if message.len() > MAX {
        message.truncate(MAX);
    }
    message
}

fn pending_path(config_path: &Path) -> PathBuf {
    sidecar_path(config_path, ".config-transaction.json")
}

fn last_path(config_path: &Path) -> PathBuf {
    sidecar_path(config_path, ".config-transaction.last.json")
}

fn history_path(config_path: &Path) -> PathBuf {
    sidecar_path(config_path, ".config-history.json")
}

fn sidecar_path(config_path: &Path, name: &str) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(name)
}

fn adjacent_temp_path(path: &Path, purpose: &str) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.yaml");
    parent.join(format!(
        ".{name}.{purpose}.{}.{}.{}.tmp",
        std::process::id(),
        unix_time_ms(),
        TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
    ))
}

fn write_json(path: &Path, value: &impl Serialize, max: usize) -> std::result::Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|err| format!("failed to serialize {}: {err}", path.display()))?;
    bytes.push(b'\n');
    if bytes.len() > max {
        return Err(format!("{} exceeds {max} bytes", path.display()));
    }
    atomic_replace(path, &bytes, true)
}

fn read_json<T>(path: &Path, label: &str, max: usize) -> std::result::Result<Option<T>, String>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("failed to read {label} {}: {err}", path.display())),
    };
    if bytes.len() > max {
        return Err(format!("{label} {} exceeds {max} bytes", path.display()));
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|err| format!("failed to parse {label} {}: {err}", path.display()))
}

fn remove_file_if_exists(path: &Path) -> std::result::Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => sync_parent(path),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("failed to remove {}: {err}", path.display())),
    }
}

fn atomic_replace(path: &Path, bytes: &[u8], sensitive: bool) -> std::result::Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|err| format!("failed to create directory {}: {err}", parent.display()))?;
    let temp_path = adjacent_temp_path(path, "write");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if sensitive { 0o600 } else { 0o666 });
    }
    let mut file = options
        .open(&temp_path)
        .map_err(|err| format!("failed to create {}: {err}", temp_path.display()))?;
    if !sensitive
        && let Ok(metadata) = fs::metadata(path)
        && let Err(err) = file.set_permissions(metadata.permissions())
    {
        let _ = fs::remove_file(&temp_path);
        return Err(format!(
            "failed to preserve permissions for {}: {err}",
            path.display()
        ));
    }
    if let Err(err) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!("failed to write {}: {err}", temp_path.display()));
    }
    replace_file(&temp_path, path).map_err(|err| {
        let _ = fs::remove_file(&temp_path);
        format!("failed to replace {}: {err}", path.display())
    })?;
    sync_parent(path)
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    use windows::core::PCWSTR;
    let from = from
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let to = to
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(std::io::Error::other)
}

fn sync_parent(path: &Path) -> std::result::Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(|err| format!("failed to sync {}: {err}", parent.display()))
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn transaction_round_trip_and_restore_preview() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.yaml");
        let previous = "plugins: []\n";
        let candidate = "plugins:\n  - tag: main\n    type: sequence\n    args:\n      exec: []\n";
        fs::write(&path, previous).unwrap();
        let record = begin(
            &path,
            previous,
            candidate,
            &config_version(previous),
            &config_version(candidate),
        )
        .unwrap();
        assert_eq!(record.status, TransactionStatus::Pending);
        assert_eq!(fs::read_to_string(&path).unwrap(), candidate);
        assert_eq!(
            pending_path(&path),
            dir.path().join(".config-transaction.json")
        );
        assert!(pending_path(&path).is_file());
        finalize(&path).unwrap();
        assert!(!pending_path(&path).exists());
        assert!(last_path(&path).is_file());
        assert!(history_path(&path).is_file());
        let history = history(&path).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, candidate);
    }

    #[test]
    fn interrupted_transaction_restores_healthy_yaml() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.yaml");
        let previous = "plugins: []\n";
        let candidate = "plugins:\n  - tag: main\n    type: sequence\n    args:\n      exec: []\n";
        fs::write(&path, previous).unwrap();
        begin(
            &path,
            previous,
            candidate,
            &config_version(previous),
            &config_version(candidate),
        )
        .unwrap();
        assert!(recover_pending(&path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), previous);
        assert_eq!(
            status(&path).unwrap().unwrap().status,
            TransactionStatus::Recovered
        );
    }

    #[test]
    fn candidate_validation_uses_the_real_config_directory() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(dir.path().join("plugins.yaml"), "plugins: []\n").unwrap();
        let summary = validate_candidate(&path, "include:\n  - plugins.yaml\nplugins: []\n")
            .expect("relative include should resolve beside the real config");
        assert_eq!(summary.plugin_count, 0);
    }

    #[test]
    fn healthy_history_deduplicates_and_evicts_oldest_entries() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.yaml");
        for index in 0..25 {
            record_initial_healthy(&path, &format!("plugins: []\n# version {index}\n")).unwrap();
        }
        record_initial_healthy(&path, "plugins: []\n# version 24\n").unwrap();
        let entries = history(&path).unwrap();
        assert_eq!(entries.len(), MAX_HISTORY_ENTRIES);
        assert_eq!(entries[0].content, "plugins: []\n# version 24\n");
        assert!(
            entries
                .iter()
                .all(|entry| entry.content != "plugins: []\n# version 0\n")
        );
    }

    #[test]
    fn rollback_restores_the_runtime_healthy_yaml_not_the_saved_base() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.yaml");
        let healthy = "plugins: []\n# running\n";
        let saved = "plugins: []\n# saved only\n";
        let candidate = "plugins: []\n# candidate\n";
        fs::write(&path, saved).unwrap();
        begin(
            &path,
            healthy,
            candidate,
            &config_version(saved),
            &config_version(candidate),
        )
        .unwrap();
        assert!(rollback(&path, "assembly failed").unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), healthy);
        assert_eq!(
            status(&path).unwrap().unwrap().status,
            TransactionStatus::Failed
        );
    }
}
