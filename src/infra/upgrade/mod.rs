// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Release upgrade support shared by the CLI and executor plugin.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use tracing::{info, warn};

use crate::infra::VERSION;
use crate::infra::error::{DnsError, Result};

mod archive;
mod download;
mod install;
mod model;
mod progress;
mod release;

#[cfg(not(windows))]
use archive::{find_extracted_binary, unpack_tar_gz};
#[cfg(windows)]
use archive::{find_extracted_binary_windows, unpack_zip};
pub(crate) use download::download;
#[cfg(not(windows))]
use install::replace_binary;
#[cfg(windows)]
use install::replace_binary_windows;
use install::{find_extracted_webui, replace_webui};
pub use model::{
    ApplyDecision, ApplyOutcome, ApplyRunOutcome, UpgradeBundle, UpgradeCheck, UpgradeConfig,
    UpgradeContext, UpgradeDownload,
};
pub(crate) use progress::UpgradeDownloadProgressReporter;
use release::{fetch_release, is_newer_version, select_asset};

pub async fn check(config: &UpgradeConfig) -> Result<UpgradeCheck> {
    let release = fetch_release(config).await?;
    let asset = select_asset(config, &release)?;
    let current_version = VERSION.to_string();
    let latest_version = release.version_string();
    let update_available = is_newer_version(&latest_version, &current_version);
    Ok(UpgradeCheck {
        current_version,
        latest_version,
        update_available,
        asset_name: asset.name.clone(),
        release_url: release.html_url.unwrap_or_default(),
    })
}

pub async fn should_apply(config: &UpgradeConfig) -> Result<ApplyDecision> {
    let check = check(config).await?;
    if config.force || check.update_available {
        Ok(ApplyDecision::Apply { check })
    } else {
        Ok(ApplyDecision::Skip { check })
    }
}

pub async fn apply(config: &UpgradeConfig, context: UpgradeContext) -> Result<ApplyRunOutcome> {
    let decision = should_apply(config).await?;
    apply_decision(config, context, decision).await
}

pub async fn apply_decision(
    config: &UpgradeConfig,
    context: UpgradeContext,
    decision: ApplyDecision,
) -> Result<ApplyRunOutcome> {
    match decision {
        ApplyDecision::Apply { check } => {
            let outcome = apply_unchecked(config, context).await?;
            Ok(ApplyRunOutcome::Applied { check, outcome })
        }
        ApplyDecision::Skip { check } => Ok(ApplyRunOutcome::Skipped { check }),
    }
}

pub(crate) async fn apply_unchecked(
    config: &UpgradeConfig,
    context: UpgradeContext,
) -> Result<ApplyOutcome> {
    print_cli_apply_step(context, "Acquiring upgrade lock...");
    let lock_path = config.cache_dir.join(".upgrade.lock");
    fs::create_dir_all(&config.cache_dir)?;
    let lock_file = File::create(&lock_path).map_err(|err| {
        DnsError::runtime(format!(
            "failed to create upgrade lock '{}': {}",
            lock_path.display(),
            err
        ))
    })?;
    lock_file.try_lock_exclusive().map_err(|err| {
        DnsError::runtime(format!("another upgrade appears to be running: {err}"))
    })?;

    print_cli_apply_step(
        context,
        "Downloading archive and verifying GitHub asset digest...",
    );
    let progress_reporter = UpgradeDownloadProgressReporter::new(context);
    let downloaded = download(config, move |progress| {
        progress_reporter.report(progress);
    })
    .await?;
    print_cli_apply_step(
        context,
        format!(
            "Archive ready: {} (sha256 {})",
            downloaded.archive_path.display(),
            downloaded.sha256
        ),
    );

    #[cfg(not(windows))]
    if !downloaded.asset_name.ends_with(".tar.gz") {
        return Err(DnsError::runtime(format!(
            "upgrade apply requires a .tar.gz asset, got '{}'",
            downloaded.asset_name
        )));
    }
    #[cfg(windows)]
    if !downloaded.asset_name.ends_with(".zip") {
        return Err(DnsError::runtime(format!(
            "upgrade apply requires a .zip asset, got '{}'",
            downloaded.asset_name
        )));
    }

    let unpack_dir = config.cache_dir.join(format!(
        ".unpack-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    ));
    if unpack_dir.exists() {
        fs::remove_dir_all(&unpack_dir)?;
    }
    fs::create_dir_all(&unpack_dir)?;
    print_cli_apply_step(
        context,
        format!("Unpacking archive into {}...", unpack_dir.display()),
    );
    #[cfg(not(windows))]
    unpack_tar_gz(&downloaded.archive_path, &unpack_dir)?;
    #[cfg(windows)]
    unpack_zip(&downloaded.archive_path, &unpack_dir)?;

    #[cfg(not(windows))]
    let extracted = find_extracted_binary(&unpack_dir)?;
    #[cfg(windows)]
    let extracted = find_extracted_binary_windows(&unpack_dir)?;

    let current_exe = std::env::current_exe()
        .map_err(|err| DnsError::runtime(format!("failed to resolve current exe: {err}")))?;
    fs::create_dir_all(&config.backup_dir)?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    #[cfg(not(windows))]
    let backup_path = config.backup_dir.join(format!("oxidns-{}-{}", VERSION, ts));
    #[cfg(windows)]
    let backup_path = config
        .backup_dir
        .join(format!("oxidns-{}-{}.exe", VERSION, ts));

    print_cli_apply_step(
        context,
        format!("Creating backup at {}...", backup_path.display()),
    );
    print_cli_apply_step(
        context,
        format!("Replacing binary at {}...", current_exe.display()),
    );
    #[cfg(not(windows))]
    {
        fs::copy(&current_exe, &backup_path).map_err(|err| {
            DnsError::runtime(format!(
                "failed to create binary backup '{}': {}",
                backup_path.display(),
                err
            ))
        })?;
        if let Err(err) = replace_binary(&extracted, &current_exe) {
            let _ = fs::copy(&backup_path, &current_exe);
            return Err(err);
        }
    }
    // Windows: rename running exe to backup then place new binary at original
    // path. replace_binary_windows() handles backup creation and rollback
    // atomically.
    #[cfg(windows)]
    replace_binary_windows(&extracted, &current_exe, &backup_path)?;
    print_cli_apply_step(context, "Binary replacement completed.");

    let (webui_path, webui_backup_path) = if config.skip_webui {
        print_cli_apply_step(context, "Skipping WebUI upgrade (--skip-webui).");
        (None, None)
    } else {
        match find_extracted_webui(&unpack_dir) {
            None => {
                print_cli_apply_step(
                    context,
                    "Archive contains no webui directory; skipping WebUI upgrade.",
                );
                (None, None)
            }
            Some(src) => {
                print_cli_apply_step(
                    context,
                    format!("Installing WebUI into {}...", config.webui_dir.display()),
                );
                let (path, backup) = replace_webui(
                    &src,
                    &config.webui_dir,
                    &config.backup_dir,
                    &downloaded.version,
                )?;
                print_cli_apply_step(context, "WebUI upgrade completed.");
                (Some(path), backup)
            }
        }
    };

    if config.cleanup_after_apply {
        if let Err(err) = FileExt::unlock(&lock_file) {
            warn!(error = %err, "failed to release upgrade lock before cleanup");
        }
        drop(lock_file);
        if let Err(err) = cleanup_upgrade_artifacts(config) {
            warn!(error = %err, "failed to clean upgrade artifacts");
        }
    }

    Ok(ApplyOutcome {
        installed_version: downloaded.version,
        asset_name: downloaded.asset_name,
        backup_path,
        binary_path: current_exe,
        restart_required: !config.no_restart,
        webui_path,
        webui_backup_path,
    })
}

pub(crate) fn cleanup_upgrade_artifacts(config: &UpgradeConfig) -> Result<Vec<PathBuf>> {
    let mut cleaned = Vec::new();
    cleanup_dir_if_exists(&config.cache_dir, &mut cleaned)?;
    if config.backup_dir != config.cache_dir {
        cleanup_dir_if_exists(&config.backup_dir, &mut cleaned)?;
    }
    Ok(cleaned)
}

fn cleanup_dir_if_exists(path: &Path, cleaned: &mut Vec<PathBuf>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    fs::remove_dir_all(path).map_err(|err| {
        DnsError::runtime(format!(
            "failed to remove upgrade directory '{}': {}",
            path.display(),
            err
        ))
    })?;
    cleaned.push(path.to_path_buf());
    Ok(())
}

fn print_cli_apply_step(context: UpgradeContext, message: impl AsRef<str>) {
    match context {
        UpgradeContext::Cli => println!("{}", message.as_ref()),
        UpgradeContext::Plugin => info!(message = message.as_ref(), "upgrade apply step"),
    }
}

#[cfg(test)]
mod tests {
    use http::header::AUTHORIZATION;

    use super::install::copy_dir_all;
    use super::release::{
        GitHubRelease, ReleaseAsset, archive_name_for_bundle, github_request_headers,
        release_target_for_bundle, resolve_requested_bundle, sha256_from_asset_digest,
    };
    use super::*;

    #[test]
    fn parses_asset_sha256_digest() {
        let asset = ReleaseAsset {
            name: "oxidns.tar.gz".to_string(),
            browser_download_url: "https://example.com/oxidns.tar.gz".to_string(),
            digest: Some(
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
            ),
        };
        let parsed = sha256_from_asset_digest(&asset).unwrap();
        assert_eq!(
            parsed,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn version_compare_handles_v_prefix() {
        assert!(is_newer_version("v0.4.2", "0.4.1"));
        assert!(!is_newer_version("v0.4.1", "0.4.1"));
    }

    #[test]
    fn github_request_headers_include_authorization_when_token_is_set() {
        let headers = github_request_headers(Some(" ghp_test "));
        assert!(headers.iter().any(|(name, value)| {
            *name == AUTHORIZATION && value.to_str().unwrap() == "Bearer ghp_test"
        }));
    }

    #[test]
    fn github_request_headers_skip_authorization_when_token_is_empty() {
        let headers = github_request_headers(Some("   "));
        assert!(!headers.iter().any(|(name, _)| *name == AUTHORIZATION));
    }

    #[test]
    fn archive_name_for_full_bundle_uses_legacy_name() {
        let name =
            archive_name_for_bundle(UpgradeBundle::Full, "x86_64-unknown-linux-musl", "tar.gz")
                .unwrap();

        assert_eq!(name, "oxidns-x86_64-unknown-linux-musl.tar.gz");
    }

    #[test]
    fn archive_name_for_slim_bundles_uses_prefixed_name() {
        let minimal = archive_name_for_bundle(
            UpgradeBundle::Minimal,
            "x86_64-unknown-linux-musl",
            "tar.gz",
        )
        .unwrap();
        let standard = archive_name_for_bundle(
            UpgradeBundle::Standard,
            "aarch64-unknown-linux-musl",
            "tar.gz",
        )
        .unwrap();

        assert_eq!(minimal, "oxidns-minimal-x86_64-unknown-linux-musl.tar.gz");
        assert_eq!(
            standard,
            "oxidns-standard-aarch64-unknown-linux-musl.tar.gz"
        );
    }

    #[test]
    fn release_target_for_slim_bundles_uses_published_linux_musl_assets() {
        let x86_64 = release_target_for_bundle(
            UpgradeBundle::Standard,
            "x86_64-unknown-linux-gnu".to_string(),
        );
        let aarch64 = release_target_for_bundle(
            UpgradeBundle::Minimal,
            "aarch64-unknown-linux-gnu".to_string(),
        );

        assert_eq!(x86_64, "x86_64-unknown-linux-musl");
        assert_eq!(aarch64, "aarch64-unknown-linux-musl");
    }

    #[test]
    fn release_target_for_full_bundle_uses_published_32_bit_linux_musl_assets() {
        let cases = [
            ("i686-unknown-linux-gnu", "i686-unknown-linux-musl"),
            (
                "arm-unknown-linux-gnueabihf",
                "arm-unknown-linux-musleabihf",
            ),
            (
                "armv7-unknown-linux-gnueabihf",
                "armv7-unknown-linux-musleabihf",
            ),
        ];

        for (source, expected) in cases {
            let target = release_target_for_bundle(UpgradeBundle::Full, source.to_string());

            assert_eq!(target, expected);
        }
    }

    #[test]
    fn release_target_for_full_bundle_uses_published_windows_msvc_assets() {
        let cases = [
            ("x86_64-pc-windows-gnu", "x86_64-pc-windows-msvc"),
            ("x86_64-pc-windows-gnullvm", "x86_64-pc-windows-msvc"),
            ("i686-pc-windows-gnu", "i686-pc-windows-msvc"),
            ("i686-pc-windows-gnullvm", "i686-pc-windows-msvc"),
            ("aarch64-pc-windows-gnullvm", "aarch64-pc-windows-msvc"),
        ];

        for (source, expected) in cases {
            let target = release_target_for_bundle(UpgradeBundle::Full, source.to_string());

            assert_eq!(target, expected);
        }
    }

    #[test]
    fn release_target_for_full_bundle_preserves_published_linux_gnu_assets() {
        let x86_64 =
            release_target_for_bundle(UpgradeBundle::Full, "x86_64-unknown-linux-gnu".to_string());
        let aarch64 =
            release_target_for_bundle(UpgradeBundle::Full, "aarch64-unknown-linux-gnu".to_string());

        assert_eq!(x86_64, "x86_64-unknown-linux-gnu");
        assert_eq!(aarch64, "aarch64-unknown-linux-gnu");
    }

    #[test]
    fn auto_bundle_resolves_from_primary_bundle() {
        assert_eq!(
            resolve_requested_bundle(UpgradeBundle::Auto, "standard").unwrap(),
            UpgradeBundle::Standard
        );
        assert_eq!(
            resolve_requested_bundle(UpgradeBundle::Auto, "minimal").unwrap(),
            UpgradeBundle::Minimal
        );
        assert_eq!(
            resolve_requested_bundle(UpgradeBundle::Auto, "full").unwrap(),
            UpgradeBundle::Full
        );
    }

    #[test]
    fn auto_bundle_rejects_custom_builds() {
        let err = resolve_requested_bundle(UpgradeBundle::Auto, "custom").unwrap_err();

        assert!(err.to_string().contains("current build bundle is custom"));
        assert!(err.to_string().contains("--asset"));
    }

    #[test]
    fn explicit_asset_overrides_bundle_selection() {
        let release = GitHubRelease {
            tag_name: "v1.2.3".to_string(),
            prerelease: false,
            html_url: None,
            assets: vec![
                ReleaseAsset {
                    name: "oxidns-standard-x86_64-unknown-linux-musl.tar.gz".to_string(),
                    browser_download_url: "https://example.com/standard.tar.gz".to_string(),
                    digest: None,
                },
                ReleaseAsset {
                    name: "custom.tar.gz".to_string(),
                    browser_download_url: "https://example.com/custom.tar.gz".to_string(),
                    digest: None,
                },
            ],
        };
        let config = UpgradeConfig {
            asset: "custom.tar.gz".to_string(),
            bundle: UpgradeBundle::Standard,
            ..UpgradeConfig::default()
        };

        let asset = select_asset(&config, &release).unwrap();

        assert_eq!(asset.name, "custom.tar.gz");
    }

    #[test]
    fn config_default_has_webui_defaults() {
        let config = UpgradeConfig::default();
        assert_eq!(config.webui_dir, PathBuf::from("./webui"));
        assert!(!config.skip_webui);
        assert!(!config.no_restart);
        assert_eq!(config.bundle, UpgradeBundle::Auto);
    }

    #[test]
    fn cleanup_upgrade_artifacts_removes_cache_and_backups() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        let backup_dir = tmp.path().join("backups");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::create_dir_all(&backup_dir).unwrap();
        fs::write(cache_dir.join("archive.tmp"), b"cache").unwrap();
        fs::write(backup_dir.join("oxidns.old"), b"backup").unwrap();
        let config = UpgradeConfig {
            cache_dir: cache_dir.clone(),
            backup_dir: backup_dir.clone(),
            ..UpgradeConfig::default()
        };

        let cleaned = cleanup_upgrade_artifacts(&config).unwrap();

        assert_eq!(cleaned, vec![cache_dir.clone(), backup_dir.clone()]);
        assert!(!cache_dir.exists());
        assert!(!backup_dir.exists());
    }

    #[cfg(not(windows))]
    fn write_file(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    #[test]
    #[cfg(not(windows))]
    fn copy_dir_all_copies_nested_tree() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        write_file(&src.join("index.html"), b"index");
        write_file(&src.join("_next/static/a.js"), b"chunk");
        fs::create_dir_all(src.join("empty")).unwrap();

        copy_dir_all(&src, &dst).unwrap();

        assert_eq!(fs::read(dst.join("index.html")).unwrap(), b"index");
        assert_eq!(fs::read(dst.join("_next/static/a.js")).unwrap(), b"chunk");
        assert!(dst.join("empty").is_dir());
    }

    #[test]
    #[cfg(not(windows))]
    fn find_extracted_webui_detects_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(find_extracted_webui(tmp.path()).is_none());
        write_file(&tmp.path().join("webui").join("index.html"), b"x");
        assert_eq!(
            find_extracted_webui(tmp.path()),
            Some(tmp.path().join("webui"))
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn replace_webui_fresh_install_no_backup() {
        let tmp = tempfile::TempDir::new().unwrap();
        let unpacked = tmp.path().join(".unpack/webui");
        write_file(&unpacked.join("index.html"), b"new");
        let target = tmp.path().join("nested/served/webui");
        let backup_dir = tmp.path().join("backups");

        let (installed, backup) = replace_webui(&unpacked, &target, &backup_dir, "0.6.0").unwrap();

        assert_eq!(installed, target);
        assert!(backup.is_none());
        assert_eq!(fs::read(target.join("index.html")).unwrap(), b"new");
    }

    #[test]
    #[cfg(not(windows))]
    fn replace_webui_backs_up_and_swaps() {
        let tmp = tempfile::TempDir::new().unwrap();
        let unpacked = tmp.path().join(".unpack/webui");
        write_file(&unpacked.join("index.html"), b"new-content");
        let target = tmp.path().join("webui");
        write_file(&target.join("marker.txt"), b"old-marker");
        let backup_dir = tmp.path().join("backups");

        let (installed, backup) = replace_webui(&unpacked, &target, &backup_dir, "0.6.0").unwrap();

        assert_eq!(installed, target);
        assert_eq!(fs::read(target.join("index.html")).unwrap(), b"new-content");
        assert!(!target.join("marker.txt").exists());
        let backup = backup.expect("existing webui must be backed up");
        assert!(backup.starts_with(&backup_dir));
        assert_eq!(fs::read(backup.join("marker.txt")).unwrap(), b"old-marker");
    }

    #[test]
    #[cfg(unix)]
    fn replace_webui_updates_symlink_target_without_replacing_link() {
        let tmp = tempfile::TempDir::new().unwrap();
        let unpacked = tmp.path().join(".unpack/webui");
        write_file(&unpacked.join("index.html"), b"new-content");
        let real_target = tmp.path().join("usr/share/oxidns/webui");
        write_file(&real_target.join("marker.txt"), b"old-marker");
        let link_parent = tmp.path().join("var/lib/oxidns");
        fs::create_dir_all(&link_parent).unwrap();
        let link_target = link_parent.join("webui");
        std::os::unix::fs::symlink(&real_target, &link_target).unwrap();
        let backup_dir = tmp.path().join("backups");

        let (installed, backup) =
            replace_webui(&unpacked, &link_target, &backup_dir, "0.6.0").unwrap();

        assert_eq!(installed, fs::canonicalize(&real_target).unwrap());
        assert!(
            fs::symlink_metadata(&link_target)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read(link_target.join("index.html")).unwrap(),
            b"new-content"
        );
        assert!(!link_target.join("marker.txt").exists());
        let backup = backup.expect("existing symlink target must be backed up");
        assert_eq!(fs::read(backup.join("marker.txt")).unwrap(), b"old-marker");
    }
}
