// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;

use super::rules::{DynamicDomainRuleKind, canonicalize_rule};
use crate::core::error::{DnsError, Result as DnsResult};

pub(super) fn read_rule_file(path: &Path) -> DnsResult<Vec<String>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(path).map_err(|err| {
        DnsError::plugin(format!(
            "failed to open dynamic_domain_set file '{}': {}",
            path.display(),
            err
        ))
    })?;
    let reader = BufReader::with_capacity(256 * 1024, file);
    let mut rules = Vec::new();
    let mut seen = HashSet::new();
    for (line_idx, line) in reader.lines().enumerate() {
        let line = line.map_err(|err| {
            DnsError::plugin(format!(
                "failed to read dynamic_domain_set file '{}' at line {}: {}",
                path.display(),
                line_idx + 1,
                err
            ))
        })?;
        let raw = line.trim();
        if raw.is_empty() || raw.starts_with('#') {
            continue;
        }
        // Existing text files follow `domain_set` semantics: bare domains mean
        // suffix-domain rules. Auto-learned exact rules are written with the
        // explicit `full:` prefix, so this default does not affect them.
        let rule = canonicalize_rule(
            raw,
            DynamicDomainRuleKind::Domain,
            &format!("file '{}', line {}", path.display(), line_idx + 1),
        )?;
        if seen.insert(rule.clone()) {
            rules.push(rule);
        }
    }
    Ok(rules)
}

pub(super) fn append_rule_file(path: &Path, rules: &[String]) -> DnsResult<()> {
    if rules.is_empty() {
        return Ok(());
    }
    // Append is used only for newly staged rules. Full rewrites are reserved
    // for delete/clear so the common learn path avoids rewriting large files.
    with_rule_file_lock(path, || {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(path)?;
        let file_len = file.metadata()?.len();
        if file_len > 0 {
            let mut last = [0_u8; 1];
            file.seek(SeekFrom::Start(file_len - 1))?;
            file.read_exact(&mut last)?;
            if last[0] != b'\n' {
                // External edits may leave the file without a trailing newline.
                // Separate the first appended rule from the previous line so a
                // later reload sees the same canonical rules the hot snapshot
                // already contains.
                writeln!(file)?;
            }
        }
        for rule in rules {
            writeln!(file, "{rule}")?;
        }
        file.sync_all()?;
        Ok(())
    })
}

pub(super) fn rewrite_rule_file(path: &Path, rules: &[String]) -> DnsResult<()> {
    with_rule_file_lock(path, || {
        let tmp_path = temp_path_for(path);
        {
            let mut file = File::create(&tmp_path)?;
            for rule in rules {
                writeln!(file, "{rule}")?;
            }
            file.sync_all()?;
        }
        // Rename keeps readers from observing a partially rewritten file on
        // platforms where same-directory rename is atomic.
        fs::rename(&tmp_path, path)?;
        Ok(())
    })
}

fn with_rule_file_lock<F>(path: &Path, op: F) -> DnsResult<()>
where
    F: FnOnce() -> std::io::Result<()>,
{
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    // A side-car lock file avoids locking the rule file being atomically
    // renamed. It is advisory, but it reduces corruption risk when two OxiDNS
    // processes accidentally manage the same path.
    let lock_path = lock_path_for(path);
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    lock_file.lock_exclusive()?;
    let result = op();
    let unlock_result = lock_file.unlock();
    result?;
    unlock_result?;
    Ok(())
}

fn lock_path_for(path: &Path) -> PathBuf {
    let mut lock_name = path.as_os_str().to_os_string();
    lock_name.push(".lock");
    PathBuf::from(lock_name)
}

fn temp_path_for(path: &Path) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let mut tmp_name = path.as_os_str().to_os_string();
    tmp_name.push(format!(".tmp.{}.{}", std::process::id(), now));
    PathBuf::from(tmp_name)
}
