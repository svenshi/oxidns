// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runtime configuration loading and validation entry points.
//!
//! OxiDNS configuration is defined as YAML and deserialized into
//! [`types::Config`]. This module keeps the file-loading boundary small:
//!
//! - read the configuration file from disk;
//! - expand environment variable placeholders in the YAML text;
//! - deserialize it into strongly typed Rust structures; and
//! - trigger semantic validation before the runtime starts.
//!
//! The detailed schema lives in [`types`]. Keeping I/O and schema definitions
//! separate makes it easier to reuse the same validation path from the CLI,
//! tests, and future embedding scenarios.

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::types::Config;
use crate::core::error::{DnsError, Result};
use crate::plugin::DependencyGraphReport;

pub mod diagnostic;
pub mod env_expand;
pub mod types;

const MAX_INCLUDE_DEPTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigValidationSummary {
    pub plugin_count: usize,
    pub dependency_graph: DependencyGraphReport,
}

/// Load and parse configuration from YAML file
///
/// # Errors
/// Returns an error if the file cannot be read, if YAML parsing fails, or if
/// validation fails.
pub fn init(file: &Path) -> Result<Config> {
    let config = load_config_with_includes(file, 0)?;

    // Validate configuration - ConfigError is auto-converted to DnsError
    config.validate()?;
    Ok(config)
}

/// Validate configuration from an on-disk YAML file.
pub fn validate_file(path: &Path) -> Result<ConfigValidationSummary> {
    let config = init(path)?;
    let dependency_graph = crate::plugin::analyze_configuration(&config)?;
    Ok(ConfigValidationSummary {
        plugin_count: config.plugins.len(),
        dependency_graph,
    })
}

/// Validate configuration from YAML text.
pub fn validate_text(text: &str) -> Result<ConfigValidationSummary> {
    let expanded = env_expand::expand_env(text)
        .map_err(|err| DnsError::config(format!("env expansion failed: {err}")))?;
    let config: Config = serde_yaml_ng::from_str(&expanded)?;
    if !config.include.is_empty() {
        return Err(DnsError::config(
            "include is only supported when validating configuration from a file",
        ));
    }
    config.validate()?;
    let dependency_graph = crate::plugin::analyze_configuration(&config)?;
    Ok(ConfigValidationSummary {
        plugin_count: config.plugins.len(),
        dependency_graph,
    })
}

fn load_config_with_includes(path: &Path, depth: usize) -> Result<Config> {
    if depth > MAX_INCLUDE_DEPTH {
        return Err(DnsError::config(format!(
            "maximum include depth of {MAX_INCLUDE_DEPTH} exceeded while loading {}",
            path.display()
        )));
    }

    let mut config = read_config(path)?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new(""));
    let mut plugins = Vec::new();

    for include in &config.include {
        let include_path = resolve_include_path(base_dir, include);
        let included = load_config_with_includes(&include_path, depth + 1).map_err(|err| {
            DnsError::config(format!(
                "failed to load included config {} referenced from {}: {}",
                include_path.display(),
                path.display(),
                err
            ))
        })?;
        plugins.extend(included.plugins);
    }

    plugins.extend(config.plugins);
    config.include.clear();
    config.plugins = plugins;
    Ok(config)
}

fn read_config(path: &Path) -> Result<Config> {
    let string = fs::read_to_string(path).map_err(|err| {
        DnsError::config(format!("failed to read config {}: {}", path.display(), err))
    })?;
    let expanded = env_expand::expand_env(&string).map_err(|err| {
        DnsError::config(format!(
            "env expansion failed in {}: {}",
            path.display(),
            err
        ))
    })?;
    serde_yaml_ng::from_str(&expanded).map_err(|err| {
        DnsError::config(format!(
            "failed to parse config {}: {}",
            path.display(),
            err
        ))
    })
}

fn resolve_include_path(base_dir: &Path, include: &str) -> PathBuf {
    let include_path = Path::new(include);
    if include_path.is_absolute() {
        include_path.to_path_buf()
    } else {
        base_dir.join(include_path)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use tempfile::{Builder, NamedTempFile, TempDir};

    use super::*;

    fn valid_config_yaml() -> &'static str {
        r#"
plugins:
  - tag: debug_main
    type: debug_print
"#
    }

    fn yaml_path(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    fn existing_env_path_root() -> (&'static str, PathBuf) {
        for name in ["TMPDIR", "HOME", "USERPROFILE"] {
            if let Some(value) = crate::core::env::var_os(name) {
                let path = PathBuf::from(value);
                if path.is_dir() {
                    return (name, path);
                }
            }
        }

        panic!("expected TMPDIR, HOME, or USERPROFILE to point to an existing directory");
    }

    #[test]
    fn validate_file_accepts_valid_config() {
        let temp = NamedTempFile::new().expect("temp file");
        std::fs::write(temp.path(), valid_config_yaml()).expect("write config");

        let summary = validate_file(temp.path()).expect("valid config should pass");
        assert_eq!(summary.plugin_count, 1);
        assert_eq!(
            summary.dependency_graph.init_order,
            vec!["debug_main".to_string()]
        );
    }

    #[test]
    fn validate_file_rejects_invalid_yaml() {
        let temp = NamedTempFile::new().expect("temp file");
        std::fs::write(temp.path(), "plugins: [").expect("write config");

        assert!(validate_file(temp.path()).is_err());
    }

    #[test]
    fn validate_text_rejects_unknown_plugin_type() {
        let err = validate_text(
            r#"
plugins:
  - tag: bad
    type: missing_plugin
"#,
        )
        .expect_err("unknown plugin should fail");

        assert!(err.to_string().contains("Unknown plugin type"));
    }

    #[test]
    fn validate_text_expands_env_vars() {
        let expected_path =
            crate::core::env::var_lossy("PATH").expect("PATH should exist in test environment");
        let summary = validate_text(
            r#"
plugins:
  - tag: '${PATH}'
    type: debug_print
"#,
        )
        .expect("PATH placeholder should expand");

        assert_eq!(summary.plugin_count, 1);
        assert_eq!(summary.dependency_graph.init_order, vec![expected_path]);
    }

    #[test]
    fn validate_text_supports_default_value() {
        let summary = validate_text(
            r#"
plugins:
  - tag: debug_main
    type: ${OXIDNS_MISSING_VALIDATE_TEXT_DEFAULT_7485F1D6:-debug_print}
"#,
        )
        .expect("default value should be used");

        assert_eq!(summary.plugin_count, 1);
        assert_eq!(
            summary.dependency_graph.init_order,
            vec!["debug_main".to_string()]
        );
    }

    #[test]
    fn validate_text_rejects_missing_env_var() {
        let err = validate_text(
            r#"
plugins:
  - tag: debug_main
    type: ${OXIDNS_MISSING_VALIDATE_TEXT_REQUIRED_D6D7F2AE}
"#,
        )
        .expect_err("missing environment variable should fail");
        let msg = err.to_string();
        assert!(msg.contains("env expansion failed"));
        assert!(msg.contains("OXIDNS_MISSING_VALIDATE_TEXT_REQUIRED_D6D7F2AE"));
    }

    #[test]
    fn validate_file_loads_included_plugins_before_main_plugins() {
        let dir = TempDir::new().expect("temp dir");
        let included_path = dir.path().join("included.yaml");
        std::fs::write(
            &included_path,
            r#"
plugins:
  - tag: included_debug
    type: debug_print
"#,
        )
        .expect("write included config");
        let main_path = dir.path().join("config.yaml");
        std::fs::write(
            &main_path,
            r#"
include:
  - included.yaml
plugins:
  - tag: main_debug
    type: debug_print
"#,
        )
        .expect("write main config");

        let summary = validate_file(&main_path).expect("included config should pass");
        assert_eq!(summary.plugin_count, 2);
        assert_eq!(
            summary.dependency_graph.init_order,
            vec!["included_debug".to_string(), "main_debug".to_string()]
        );
    }

    #[test]
    fn validate_file_allows_main_plugins_to_depend_on_included_plugins() {
        let dir = TempDir::new().expect("temp dir");
        std::fs::write(
            dir.path().join("matchers.yaml"),
            r#"
plugins:
  - tag: match_example
    type: qname
    args:
      - full:example.com
"#,
        )
        .expect("write include config");
        let main_path = dir.path().join("config.yaml");
        std::fs::write(
            &main_path,
            r#"
include:
  - matchers.yaml
plugins:
  - tag: seq
    type: sequence
    args:
      - matches:
          - match_example
        exec: accept
"#,
        )
        .expect("write main config");

        let summary = validate_file(&main_path).expect("dependency graph should resolve");
        assert_eq!(summary.plugin_count, 2);
        assert_eq!(
            summary.dependency_graph.init_order,
            vec!["match_example".to_string(), "seq".to_string()]
        );
    }

    #[test]
    fn validate_file_resolves_nested_relative_include_paths_from_declaring_file() {
        let dir = TempDir::new().expect("temp dir");
        let nested_dir = dir.path().join("nested");
        std::fs::create_dir_all(&nested_dir).expect("create nested dir");
        std::fs::write(
            nested_dir.join("leaf.yaml"),
            r#"
plugins:
  - tag: leaf_debug
    type: debug_print
"#,
        )
        .expect("write leaf config");
        std::fs::write(
            nested_dir.join("middle.yaml"),
            r#"
include:
  - leaf.yaml
plugins:
  - tag: middle_debug
    type: debug_print
"#,
        )
        .expect("write middle config");
        let main_path = dir.path().join("config.yaml");
        std::fs::write(
            &main_path,
            r#"
include:
  - nested/middle.yaml
plugins:
  - tag: main_debug
    type: debug_print
"#,
        )
        .expect("write main config");

        let config = init(&main_path).expect("nested include should pass");
        let tags = config
            .plugins
            .iter()
            .map(|plugin| plugin.tag.as_str())
            .collect::<Vec<_>>();
        assert_eq!(tags, vec!["leaf_debug", "middle_debug", "main_debug"]);
    }

    #[test]
    fn validate_file_rejects_include_depth_over_limit() {
        let dir = TempDir::new().expect("temp dir");
        for idx in 0..=MAX_INCLUDE_DEPTH {
            let next = idx + 1;
            std::fs::write(
                dir.path().join(format!("config{idx}.yaml")),
                format!(
                    r#"
include:
  - config{next}.yaml
plugins: []
"#
                ),
            )
            .expect("write config");
        }
        std::fs::write(
            dir.path()
                .join(format!("config{}.yaml", MAX_INCLUDE_DEPTH + 1)),
            "plugins: []\n",
        )
        .expect("write deepest config");

        let err = validate_file(&dir.path().join("config0.yaml"))
            .expect_err("too many includes should fail");
        assert!(err.to_string().contains("maximum include depth"));
    }

    #[test]
    fn validate_file_reports_missing_include_context() {
        let dir = TempDir::new().expect("temp dir");
        let main_path = dir.path().join("config.yaml");
        std::fs::write(
            &main_path,
            r#"
include:
  - missing.yaml
plugins: []
"#,
        )
        .expect("write main config");

        let err = validate_file(&main_path).expect_err("missing include should fail");
        let msg = err.to_string();
        assert!(msg.contains("missing.yaml"));
        assert!(msg.contains("referenced from"));
    }

    #[test]
    fn validate_file_rejects_duplicate_tags_after_include_merge() {
        let dir = TempDir::new().expect("temp dir");
        std::fs::write(
            dir.path().join("included.yaml"),
            r#"
plugins:
  - tag: dup
    type: debug_print
"#,
        )
        .expect("write include config");
        let main_path = dir.path().join("config.yaml");
        std::fs::write(
            &main_path,
            r#"
include:
  - included.yaml
plugins:
  - tag: dup
    type: debug_print
"#,
        )
        .expect("write main config");

        let err = validate_file(&main_path).expect_err("duplicate tag should fail");
        assert!(err.to_string().contains("Duplicate plugin tag 'dup'"));
    }

    #[test]
    fn validate_file_expands_env_in_include_path() {
        let (env_name, root) = existing_env_path_root();
        let dir = Builder::new()
            .prefix("oxidns-env-include-")
            .tempdir_in(&root)
            .expect("temp dir under env root");
        std::fs::write(
            dir.path().join("included.yaml"),
            r#"
plugins:
  - tag: included_debug
    type: debug_print
"#,
        )
        .expect("write included config");

        let suffix = dir.path().strip_prefix(&root).expect("strip env root");
        let include_path = if suffix.as_os_str().is_empty() {
            format!("${{{env_name}}}/included.yaml")
        } else {
            format!("${{{env_name}}}/{}/included.yaml", yaml_path(suffix))
        };
        let main_path = dir.path().join("config.yaml");
        std::fs::write(
            &main_path,
            format!(
                r#"
include:
  - '{}'
plugins:
  - tag: main_debug
    type: debug_print
"#,
                include_path
            ),
        )
        .expect("write main config");

        let summary = validate_file(&main_path).expect("include path should expand");
        assert_eq!(summary.plugin_count, 2);
        assert_eq!(
            summary.dependency_graph.init_order,
            vec!["included_debug".to_string(), "main_debug".to_string()]
        );
    }

    #[test]
    fn validate_text_accepts_empty_include() {
        let summary = validate_text(
            r#"
include: []
plugins:
  - tag: debug_main
    type: debug_print
"#,
        )
        .expect("empty include should pass text validation");
        assert_eq!(summary.plugin_count, 1);
    }

    #[test]
    fn validate_text_rejects_non_empty_include() {
        let err = validate_text(
            r#"
include:
  - other.yaml
plugins: []
"#,
        )
        .expect_err("text validation cannot resolve include paths");
        assert!(err.to_string().contains("include is only supported"));
    }
}
