// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared helpers for the live netlink integration tests.
//!
//! - Skip with a useful message if the required cli tools aren't on PATH (e.g.
//!   running as non-root or in a container).
//! - Generate unique table/set names so parallel tests can't collide.
//! - RAII guards that destroy nftables tables / ipsets even when a test panics,
//!   so back-to-back runs don't accumulate kernel state.

#![cfg(target_os = "linux")]
#![allow(dead_code)]

use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// Generate a process-unique name with the given prefix. Collisions
/// across separate `cargo test` invocations are prevented by mixing in
/// the PID, and a counter rules out collisions within a single run.
pub fn unique_name(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    format!("{prefix}_{pid}_{n}")
}

fn run_cli(program: &str, args: &[&str]) -> Output {
    Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn `{program}`: {e}"))
}

/// Run an `nft` command, asserting success. Prints stderr on failure
/// so test output points at the actual kernel response.
pub fn run_nft(args: &[&str]) -> String {
    let out = run_cli("nft", args);
    if !out.status.success() {
        panic!(
            "`nft {}` failed (exit {:?}):\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Run an `ipset` command, asserting success.
pub fn run_ipset(args: &[&str]) -> String {
    let out = run_cli("ipset", args);
    if !out.status.success() {
        panic!(
            "`ipset {}` failed (exit {:?}):\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Sanity check that nftables tooling is reachable. If it isn't, the
/// test bails with a clear message — the user is almost certainly
/// running without `sudo` or on a host without nftables installed.
pub fn ensure_nft_available() {
    let out = Command::new("nft").arg("--version").output();
    match out {
        Ok(o) if o.status.success() => {}
        _ => panic!(
            "`nft` is required for this test. Re-run with: \
             sudo cargo test --package oxidns-ripset --test integration -- --ignored --test-threads=1"
        ),
    }
    // Probe for create permission — kernel returns EPERM for non-root
    // attempts to mutate nftables.
    let probe = Command::new("nft")
        .args(["list", "tables"])
        .output()
        .expect("spawn nft");
    if !probe.status.success() {
        panic!(
            "`nft list tables` failed; tests require root.\nstderr: {}",
            String::from_utf8_lossy(&probe.stderr)
        );
    }
}

/// Equivalent of `ensure_nft_available` for ipset.
pub fn ensure_ipset_available() {
    let out = Command::new("ipset").arg("version").output();
    match out {
        Ok(o) if o.status.success() => {}
        _ => panic!(
            "`ipset` is required for this test. Re-run with: \
             sudo cargo test --package oxidns-ripset --test integration -- --ignored --test-threads=1"
        ),
    }
    let probe = Command::new("ipset")
        .arg("list")
        .output()
        .expect("spawn ipset");
    if !probe.status.success() {
        panic!(
            "`ipset list` failed; tests require root.\nstderr: {}",
            String::from_utf8_lossy(&probe.stderr)
        );
    }
}

/// Deletes an nftables table on drop. `family` is `ip` / `ip6` /
/// `inet`. Silent on cleanup failure — the test has already passed or
/// failed, and the table may have been deleted via the library API.
pub struct NftCleanup {
    family: String,
    table: String,
}

impl NftCleanup {
    pub fn new(family: &str, table: &str) -> Self {
        Self {
            family: family.to_string(),
            table: table.to_string(),
        }
    }
}

impl Drop for NftCleanup {
    fn drop(&mut self) {
        let _ = Command::new("nft")
            .args(["delete", "table", &self.family, &self.table])
            .output();
    }
}

/// Deletes an ipset on drop.
pub struct IpsetCleanup {
    name: String,
}

impl IpsetCleanup {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl Drop for IpsetCleanup {
    fn drop(&mut self) {
        // `destroy` removes the set; `-exist` swallows "doesn't exist".
        let _ = Command::new("ipset").args(["destroy", &self.name]).output();
    }
}
