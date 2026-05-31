// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Application CLI definition and startup options.

use std::path::PathBuf;
#[cfg(feature = "plugin-upgrade")]
use std::time::Duration;

use clap::{Args, Parser, Subcommand};

#[cfg(feature = "plugin-upgrade")]
use crate::upgrade::UpgradeBundle;

/// Top-level CLI definition.
#[derive(Parser, Clone, Debug)]
#[command(version = crate::build_info::CLI_VERSION, author = "Sven Shi <isvenshi@gmail.com>")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// Supported top-level commands.
#[derive(Subcommand, Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Start OxiDNS in the foreground.
    Start(StartOptions),
    /// Check whether a configuration file is valid.
    Check(CheckOptions),
    /// Print compiled feature and plugin capability information.
    BuildInfo,
    /// Export selected rules from a dat file into text files.
    #[cfg(feature = "provider-protobuf")]
    ExportDat(ExportDatOptions),
    /// Manage the operating system service.
    Service(ServiceOptions),
    /// Check, download, or apply OxiDNS release upgrades.
    #[cfg(feature = "plugin-upgrade")]
    Upgrade(UpgradeOptions),
}

/// Foreground start options.
#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct StartOptions {
    /// Path to configuration file
    #[arg(short = 'c', long = "config", default_value = "config.yaml")]
    pub config: PathBuf,

    /// Working directory for OxiDNS
    #[arg(short = 'd', long = "working-dir")]
    pub working_dir: Option<PathBuf>,

    /// Log level override (overrides config file): off, trace, debug, info,
    /// warn, error
    #[arg(short = 'l', long = "log-level")]
    pub log_level: Option<String>,
}

/// Static configuration check options.
#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct CheckOptions {
    /// Path to configuration file
    #[arg(short = 'c', long = "config", default_value = "config.yaml")]
    pub config: PathBuf,

    /// Working directory for resolving relative paths
    #[arg(short = 'd', long = "working-dir")]
    pub working_dir: Option<PathBuf>,

    /// Print plugin dependency graph after validation succeeds
    #[arg(long = "graph", default_value_t = false)]
    pub graph: bool,
}

/// Dat export options.
#[cfg(feature = "provider-protobuf")]
#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct ExportDatOptions {
    /// Path to the source dat file
    #[arg(long = "file")]
    pub file: PathBuf,

    /// Explicit dat kind: auto, geosite, geoip
    #[arg(long = "kind", value_enum, default_value_t = DatKind::Auto)]
    pub kind: DatKind,

    /// Output text format: oxidns or original
    #[arg(long = "format", value_enum, default_value_t = ExportFormat::Oxidns)]
    pub format: ExportFormat,

    /// Selector to export; repeat this flag to export multiple selectors
    #[arg(long = "selector")]
    pub selectors: Vec<String>,

    /// Output directory for exported files
    #[arg(long = "out-dir")]
    pub out_dir: PathBuf,

    /// Optional merged output file name written inside --out-dir
    #[arg(long = "merged-file")]
    pub merged_file: Option<String>,

    /// Allow overwriting existing output files
    #[arg(long = "overwrite", default_value_t = false)]
    pub overwrite: bool,
}

/// Supported dat kinds.
#[cfg(feature = "provider-protobuf")]
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatKind {
    Auto,
    Geosite,
    Geoip,
}

/// Supported export text formats.
#[cfg(feature = "provider-protobuf")]
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Oxidns,
    Original,
}

/// Upgrade command options.
#[cfg(feature = "plugin-upgrade")]
#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct UpgradeOptions {
    #[command(subcommand)]
    pub action: Option<UpgradeAction>,

    /// Path to runtime configuration used to infer the WebUI directory.
    #[arg(short = 'c', long = "config", global = true)]
    pub config: Option<PathBuf>,

    /// Working directory for resolving runtime-relative paths.
    #[arg(short = 'd', long = "working-dir", global = true)]
    pub working_dir: Option<PathBuf>,

    /// Release tag to use, or latest.
    #[arg(long = "target", default_value = "latest", global = true)]
    pub target: String,

    /// GitHub repository in owner/name form.
    #[arg(long = "repository", default_value = "svenshi/oxidns", global = true)]
    pub repository: String,

    /// Release asset name, or auto for the current platform and bundle archive.
    #[arg(long = "asset", default_value = "auto", global = true)]
    pub asset: String,

    /// Release build bundle to use when asset is auto.
    #[arg(long = "bundle", value_enum, default_value = "auto", global = true)]
    pub bundle: UpgradeBundle,

    /// Directory used to cache downloaded release files.
    #[arg(long = "cache-dir", default_value = "./upgrade-cache", global = true)]
    pub cache_dir: PathBuf,

    /// Directory used to store binary backups before apply.
    #[arg(
        long = "backup-dir",
        default_value = "./upgrade-backups",
        global = true
    )]
    pub backup_dir: PathBuf,

    /// Directory where the served WebUI assets are installed.
    #[arg(long = "webui-dir", global = true)]
    pub webui_dir: Option<PathBuf>,

    /// Skip upgrading the WebUI directory during apply.
    #[arg(long = "skip-webui", default_value_t = false, global = true)]
    pub skip_webui: bool,

    /// Skip restarting the service after a successful apply.
    #[arg(long = "no-restart", default_value_t = false, global = true)]
    pub no_restart: bool,

    /// Allow prerelease GitHub releases.
    #[arg(long = "allow-prerelease", default_value_t = false, global = true)]
    pub allow_prerelease: bool,

    /// Apply even when the selected release is not newer than the current
    /// version.
    #[arg(long = "force", default_value_t = false, global = true)]
    pub force: bool,

    /// Request timeout such as 30s, 2m, or 500ms.
    #[arg(long = "timeout", value_parser = parse_cli_duration, default_value = "30s", global = true)]
    pub timeout: Duration,

    /// Optional SOCKS5 proxy address.
    #[arg(long = "socks5", global = true)]
    pub socks5: Option<String>,

    /// Disable TLS certificate verification for upgrade downloads.
    #[arg(long = "insecure-skip-verify", default_value_t = false, global = true)]
    pub insecure_skip_verify: bool,

    /// GitHub personal access token for API requests.
    #[arg(long = "github-token", global = true)]
    pub github_token: Option<String>,
}

/// Upgrade subcommands.
#[cfg(feature = "plugin-upgrade")]
#[derive(Subcommand, Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeAction {
    Check,
    Download,
    Apply,
}

#[cfg(feature = "plugin-upgrade")]
fn parse_cli_duration(raw: &str) -> std::result::Result<Duration, String> {
    crate::core::system_utils::parse_simple_duration(raw)
}

/// Service command options.
#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct ServiceOptions {
    #[command(subcommand)]
    pub command: ServiceCommand,
}

/// Supported service manager actions.
#[derive(Subcommand, Clone, Debug, PartialEq, Eq)]
pub enum ServiceCommand {
    /// Install the system service. Installation only registers auto-start, it
    /// does not start immediately.
    Install(ServiceInstallOptions),
    /// Start the installed service.
    Start,
    /// Stop the installed service.
    Stop,
    /// Restart the installed service.
    Restart,
    /// Uninstall the installed service.
    Uninstall,
}

/// Service installation options.
#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct ServiceInstallOptions {
    /// Absolute working directory for the installed service.
    #[arg(short = 'd', long = "working-dir")]
    pub working_dir: PathBuf,

    /// Path to configuration file used by the installed service.
    #[arg(short = 'c', long = "config")]
    pub config: PathBuf,
}

/// Parse command-line options for OxiDNS.
pub fn parse_cli() -> Cli {
    <Cli as clap::Parser>::parse()
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::*;

    #[test]
    fn cli_version_uses_compiled_version() {
        assert_eq!(
            Cli::command().get_version(),
            Some(crate::build_info::CLI_VERSION)
        );
    }

    #[test]
    fn parse_start_command_with_explicit_flags() {
        let args = [
            "oxidns",
            "start",
            "-c",
            "custom.yaml",
            "-d",
            "/tmp/oxidns",
            "-l",
            "debug",
        ];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::Start(StartOptions {
                config: PathBuf::from("custom.yaml"),
                working_dir: Some(PathBuf::from("/tmp/oxidns")),
                log_level: Some("debug".to_string()),
            })
        );
    }

    #[test]
    fn parse_start_command_uses_default_config() {
        let args = ["oxidns", "start"];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::Start(StartOptions {
                config: PathBuf::from("config.yaml"),
                working_dir: None,
                log_level: None,
            })
        );
    }

    #[test]
    fn parse_check_command_uses_default_config() {
        let args = ["oxidns", "check"];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::Check(CheckOptions {
                config: PathBuf::from("config.yaml"),
                working_dir: None,
                graph: false,
            })
        );
    }

    #[test]
    fn parse_check_command_with_explicit_config() {
        let args = ["oxidns", "check", "-c", "custom.yaml"];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::Check(CheckOptions {
                config: PathBuf::from("custom.yaml"),
                working_dir: None,
                graph: false,
            })
        );
    }

    #[test]
    fn parse_check_command_with_working_dir() {
        let args = ["oxidns", "check", "-c", "custom.yaml", "-d", "/tmp/oxidns"];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::Check(CheckOptions {
                config: PathBuf::from("custom.yaml"),
                working_dir: Some(PathBuf::from("/tmp/oxidns")),
                graph: false,
            })
        );
    }

    #[test]
    fn parse_build_info_command() {
        let args = ["oxidns", "build-info"];

        let cli = Cli::parse_from(args);
        assert_eq!(cli.command, Command::BuildInfo);
    }

    #[cfg(feature = "plugin-upgrade")]
    #[test]
    fn parse_upgrade_apply_with_options() {
        let args = [
            "oxidns",
            "upgrade",
            "apply",
            "--target",
            "v0.4.2",
            "--repository",
            "svenshi/oxidns",
            "--asset",
            "oxidns-x86_64-unknown-linux-gnu.tar.gz",
            "--cache-dir",
            "./cache",
            "--backup-dir",
            "./backups",
            "--allow-prerelease",
            "--timeout",
            "2m",
            "--socks5",
            "127.0.0.1:1080",
            "--insecure-skip-verify",
            "--github-token",
            "ghp_test_token",
        ];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::Upgrade(UpgradeOptions {
                action: Some(UpgradeAction::Apply),
                config: None,
                working_dir: None,
                target: "v0.4.2".to_string(),
                repository: "svenshi/oxidns".to_string(),
                asset: "oxidns-x86_64-unknown-linux-gnu.tar.gz".to_string(),
                bundle: UpgradeBundle::Auto,
                cache_dir: PathBuf::from("./cache"),
                backup_dir: PathBuf::from("./backups"),
                webui_dir: None,
                skip_webui: false,
                no_restart: false,
                allow_prerelease: true,
                force: false,
                timeout: Duration::from_secs(120),
                socks5: Some("127.0.0.1:1080".to_string()),
                insecure_skip_verify: true,
                github_token: Some("ghp_test_token".to_string()),
            })
        );
    }

    #[cfg(feature = "plugin-upgrade")]
    #[test]
    fn parse_upgrade_bundle_option() {
        let args = ["oxidns", "upgrade", "check", "--bundle", "standard"];

        let cli = Cli::parse_from(args);
        assert!(matches!(
            cli.command,
            Command::Upgrade(UpgradeOptions {
                bundle: UpgradeBundle::Standard,
                ..
            })
        ));
    }

    #[cfg(feature = "plugin-upgrade")]
    #[test]
    fn parse_upgrade_rejects_unknown_bundle() {
        let args = ["oxidns", "upgrade", "check", "--bundle", "tiny"];

        assert!(Cli::try_parse_from(args).is_err());
    }

    #[cfg(feature = "plugin-upgrade")]
    #[test]
    fn parse_upgrade_no_restart_flag() {
        let args = ["oxidns", "upgrade", "apply", "--no-restart"];

        let cli = Cli::parse_from(args);
        assert!(matches!(
            cli.command,
            Command::Upgrade(UpgradeOptions {
                no_restart: true,
                ..
            })
        ));
    }

    #[cfg(feature = "plugin-upgrade")]
    #[test]
    fn parse_upgrade_defaults_to_apply_and_accepts_force() {
        let args = ["oxidns", "upgrade", "--force"];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::Upgrade(UpgradeOptions {
                action: None,
                config: None,
                working_dir: None,
                target: "latest".to_string(),
                repository: "svenshi/oxidns".to_string(),
                asset: "auto".to_string(),
                bundle: UpgradeBundle::Auto,
                cache_dir: PathBuf::from("./upgrade-cache"),
                backup_dir: PathBuf::from("./upgrade-backups"),
                webui_dir: None,
                skip_webui: false,
                no_restart: false,
                allow_prerelease: false,
                force: true,
                timeout: Duration::from_secs(30),
                socks5: None,
                insecure_skip_verify: false,
                github_token: None,
            })
        );
    }

    #[test]
    fn parse_upgrade_accepts_runtime_path_context() {
        let args = [
            "oxidns",
            "upgrade",
            "-c",
            "/etc/oxidns/config.yaml",
            "-d",
            "/var/lib/oxidns",
            "--webui-dir",
            "./webui",
        ];

        let cli = Cli::parse_from(args);
        assert!(matches!(
            cli.command,
            Command::Upgrade(UpgradeOptions {
                config: Some(config),
                working_dir: Some(working_dir),
                webui_dir: Some(webui_dir),
                ..
            }) if config.as_path() == std::path::Path::new("/etc/oxidns/config.yaml")
                && working_dir.as_path() == std::path::Path::new("/var/lib/oxidns")
                && webui_dir.as_path() == std::path::Path::new("./webui")
        ));
    }

    #[test]
    fn parse_check_command_with_graph() {
        let args = ["oxidns", "check", "--graph"];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::Check(CheckOptions {
                config: PathBuf::from("config.yaml"),
                working_dir: None,
                graph: true,
            })
        );
    }

    #[test]
    fn parse_service_install_command() {
        let args = [
            "oxidns",
            "service",
            "install",
            "-d",
            "/etc/oxidns",
            "-c",
            "/etc/oxidns/config.yaml",
        ];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::Service(ServiceOptions {
                command: ServiceCommand::Install(ServiceInstallOptions {
                    working_dir: PathBuf::from("/etc/oxidns"),
                    config: PathBuf::from("/etc/oxidns/config.yaml"),
                }),
            })
        );
    }

    #[test]
    fn parse_service_start_command() {
        let args = ["oxidns", "service", "start"];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::Service(ServiceOptions {
                command: ServiceCommand::Start,
            })
        );
    }

    #[test]
    fn parse_service_stop_command() {
        let args = ["oxidns", "service", "stop"];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::Service(ServiceOptions {
                command: ServiceCommand::Stop,
            })
        );
    }

    #[test]
    fn parse_service_restart_command() {
        let args = ["oxidns", "service", "restart"];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::Service(ServiceOptions {
                command: ServiceCommand::Restart,
            })
        );
    }

    #[test]
    fn parse_service_uninstall_command() {
        let args = ["oxidns", "service", "uninstall"];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::Service(ServiceOptions {
                command: ServiceCommand::Uninstall,
            })
        );
    }

    #[cfg(feature = "provider-protobuf")]
    #[test]
    fn parse_export_dat_command() {
        let args = [
            "oxidns",
            "export-dat",
            "--file",
            "rules/geosite.dat",
            "--selector",
            "cn",
            "--selector",
            "geolocation-!cn",
            "--out-dir",
            "/tmp/out",
            "--kind",
            "geosite",
            "--format",
            "oxidns",
            "--merged-file",
            "all.txt",
            "--overwrite",
        ];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::ExportDat(ExportDatOptions {
                file: PathBuf::from("rules/geosite.dat"),
                kind: DatKind::Geosite,
                format: ExportFormat::Oxidns,
                selectors: vec!["cn".to_string(), "geolocation-!cn".to_string()],
                out_dir: PathBuf::from("/tmp/out"),
                merged_file: Some("all.txt".to_string()),
                overwrite: true,
            })
        );
    }

    #[cfg(feature = "provider-protobuf")]
    #[test]
    fn parse_export_dat_command_without_selectors() {
        let args = [
            "oxidns",
            "export-dat",
            "--file",
            "rules/geoip.dat",
            "--out-dir",
            "/tmp/out",
        ];

        let cli = Cli::parse_from(args);
        assert_eq!(
            cli.command,
            Command::ExportDat(ExportDatOptions {
                file: PathBuf::from("rules/geoip.dat"),
                kind: DatKind::Auto,
                format: ExportFormat::Oxidns,
                selectors: Vec::new(),
                out_dir: PathBuf::from("/tmp/out"),
                merged_file: None,
                overwrite: false,
            })
        );
    }
}
