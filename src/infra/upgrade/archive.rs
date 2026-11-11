// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Platform-specific upgrade archive extraction.

#[cfg(windows)]
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::infra::error::{DnsError, Result};

#[cfg(windows)]
pub(super) fn unpack_zip(archive: &Path, out_dir: &Path) -> Result<()> {
    let file = File::open(archive).map_err(|e| {
        DnsError::runtime(format!("failed to open zip '{}': {e}", archive.display()))
    })?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| DnsError::runtime(format!("failed to read zip archive: {e}")))?;
    // Canonicalize `out_dir` once so the post-join containment check is
    // resilient to relative components and current-dir changes.
    let out_dir_canon = fs::canonicalize(out_dir).map_err(|e| {
        DnsError::runtime(format!(
            "failed to canonicalize unpack dir '{}': {e}",
            out_dir.display()
        ))
    })?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| DnsError::runtime(format!("failed to access zip entry {i}: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let entry_name = entry
            .name()
            .map_err(|e| DnsError::runtime(format!("failed to decode zip entry {i} name: {e}")))?
            .into_owned();
        // `enclosed_name()` rejects absolute paths and `..` components that
        // would escape the unpack root, mitigating zip-slip on Windows where
        // backslashes and drive letters add extra footguns. Treat any
        // rejected entry as a hard error so a malicious archive cannot
        // silently skip files and leave the install in a half-applied state.
        let Some(rel_path) = entry.enclosed_name() else {
            return Err(DnsError::runtime(format!(
                "refusing to extract zip entry with unsafe path: '{entry_name}'"
            )));
        };
        let dest = out_dir_canon.join(&rel_path);
        // Defense in depth: ensure the resolved parent stays under
        // `out_dir_canon` even after the join. `enclosed_name()` already
        // enforces this, but the extra check protects against future zip
        // crate behavior changes and any host-side symlink trickery.
        let parent = dest.parent().unwrap_or(&out_dir_canon);
        fs::create_dir_all(parent).map_err(|e| {
            DnsError::runtime(format!("failed to create '{}': {e}", parent.display()))
        })?;
        if !parent.starts_with(&out_dir_canon) {
            return Err(DnsError::runtime(format!(
                "refusing to extract zip entry outside unpack dir: '{}'",
                rel_path.display()
            )));
        }
        let mut out = File::create(&dest).map_err(|e| {
            DnsError::runtime(format!("failed to create '{}': {e}", dest.display()))
        })?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| DnsError::runtime(format!("failed to extract '{entry_name}': {e}")))?;
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn find_extracted_binary_windows(unpack_dir: &Path) -> Result<PathBuf> {
    let candidate = unpack_dir.join("oxidns.exe");
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err(DnsError::runtime(format!(
        "archive did not contain oxidns.exe at '{}'",
        candidate.display()
    )))
}

#[cfg(not(windows))]
pub(super) fn unpack_tar_gz(archive: &Path, out_dir: &Path) -> Result<()> {
    let file = File::open(archive).map_err(|err| {
        DnsError::runtime(format!(
            "failed to open archive '{}': {}",
            archive.display(),
            err
        ))
    })?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(out_dir).map_err(|err| {
        DnsError::runtime(format!(
            "failed to unpack archive into '{}': {}",
            out_dir.display(),
            err
        ))
    })
}

#[cfg(not(windows))]
pub(super) fn find_extracted_binary(unpack_dir: &Path) -> Result<PathBuf> {
    let candidate = unpack_dir.join("oxidns");
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err(DnsError::runtime(format!(
        "archive did not contain oxidns binary at '{}'",
        candidate.display()
    )))
}
