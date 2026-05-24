// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared process environment variable access helpers.
//!
//! Keep all OxiDNS environment reads behind this small boundary so startup
//! config expansion, runtime matchers, and tests use the same conversion rules.

use std::ffi::{OsStr, OsString};

/// Read a process environment variable as an OS string.
pub fn var_os<K>(key: K) -> Option<OsString>
where
    K: AsRef<OsStr>,
{
    std::env::var_os(key)
}

/// Read a process environment variable and convert it to UTF-8 lossily.
pub fn var_lossy<K>(key: K) -> Option<String>
where
    K: AsRef<OsStr>,
{
    var_os(key).map(|value| value.to_string_lossy().into_owned())
}

/// Return whether a process environment variable is defined.
pub fn exists<K>(key: K) -> bool
where
    K: AsRef<OsStr>,
{
    var_os(key).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_lossy_reads_existing_environment_variable() {
        let path = var_lossy("PATH").expect("PATH should exist in test environment");
        assert!(!path.is_empty());
    }

    #[test]
    fn exists_reports_missing_environment_variable() {
        assert!(!exists("OXIDNS_MISSING_ENV_HELPER_0B5A9C66"));
    }
}
