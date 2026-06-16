// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! CLI support for validating configuration files.

use std::path::PathBuf;

use crate::cli::CheckOptions;
use crate::config;
use crate::config::ConfigValidationSummary;
use crate::infra::error::{DnsError, Result};

pub fn run(options: CheckOptions) -> Result<()> {
    match run_check(&options) {
        Ok(summary) => {
            println!(
                "Configuration is valid: {} (plugins: {})",
                options.config.display(),
                summary.plugin_count
            );
            if options.graph {
                print_dependency_graph(&summary);
            }
            Ok(())
        }
        Err(err) => {
            let message = err.to_string();
            let location = std::fs::read_to_string(&options.config)
                .ok()
                .and_then(|text| config::diagnostic::locate_in_config(&text, &message));
            match location {
                Some(loc) => eprintln!(
                    "{}:{}:{}: error: {}",
                    options.config.display(),
                    loc.line,
                    loc.column,
                    message
                ),
                None => eprintln!("error: {message}"),
            }
            Err(err)
        }
    }
}

fn prepare_working_dir(working_dir: Option<&PathBuf>) -> Result<()> {
    if let Some(working_dir) = working_dir {
        std::env::set_current_dir(working_dir).map_err(|err| {
            DnsError::runtime(format!(
                "Failed to switch working directory to {}: {}",
                working_dir.display(),
                err
            ))
        })?;
    }
    Ok(())
}

fn run_check(options: &CheckOptions) -> Result<ConfigValidationSummary> {
    prepare_working_dir(options.working_dir.as_ref())?;
    config::validate_file(&options.config).map_err(|err| {
        DnsError::config(format!(
            "Configuration initialization failed for {}: {}",
            options.config.display(),
            err
        ))
    })
}

fn print_dependency_graph(summary: &ConfigValidationSummary) {
    println!("{}", render_dependency_graph(summary));
}

fn render_dependency_graph(summary: &ConfigValidationSummary) -> String {
    super::graph::render_dependency_graph(&summary.dependency_graph)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn write_config(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write config");
        path
    }

    #[test]
    fn run_check_accepts_valid_config() {
        let temp = TempDir::new().expect("temp dir");
        let config_path = write_config(
            temp.path(),
            "config.yaml",
            r#"
plugins:
  - tag: debug_main
    type: debug_print
"#,
        );

        let summary = run_check(&CheckOptions {
            config: config_path,
            working_dir: None,
            graph: false,
        })
        .expect("valid config should pass");

        assert_eq!(summary.plugin_count, 1);
    }

    #[test]
    fn print_dependency_graph_renders_tree_from_top_level_plugins() {
        let summary = config::validate_text(
            r#"
plugins:
  - tag: forward
    type: forward
  - tag: seq
    type: sequence
    args:
      - exec: $forward
      - exec: accept
  - tag: udp_server
    type: udp_server
    args:
      entry: seq
  - tag: tcp_server
    type: tcp_server
    args:
      entry: seq
"#,
        )
        .expect("config should validate");

        let graph = render_dependency_graph(&summary);
        assert!(graph.contains("udp_server [server:udp_server]"));
        assert!(graph.contains("tcp_server [server:tcp_server]"));
        assert!(
            graph.contains("udp_server [server:udp_server]\n\n")
                || graph.contains("tcp_server [server:tcp_server]\n\n")
        );
        assert!(graph.contains("#0 IF always"));
        assert!(graph.contains("THEN $forward [args[0].exec]"));
        assert!(graph.contains("#1 IF always"));
        assert!(graph.contains("THEN accept [args[1].exec]"));
        assert!(!graph.contains("no dependencies"));
    }

    #[test]
    fn print_dependency_graph_expands_nested_sequence_targets() {
        let summary = config::validate_text(
            r#"
plugins:
  - tag: cache
    type: cache
  - tag: child_seq
    type: sequence
    args:
      - exec: $cache
  - tag: main_seq
    type: sequence
    args:
      - exec: jump child_seq
  - tag: udp_server
    type: udp_server
    args:
      entry: main_seq
"#,
        )
        .expect("config should validate");

        let graph = render_dependency_graph(&summary);
        assert!(graph.contains("main_seq [executor:sequence]"));
        assert!(graph.contains("THEN jump child_seq [args[0].exec]"));
        assert!(graph.contains("child_seq [executor:sequence]"));
        assert!(graph.contains("THEN $cache [args[0].exec]"));
        assert!(graph.contains("cache [executor:cache]"));
    }

    #[test]
    fn print_dependency_graph_shows_quick_setup_provider_deps_under_rule() {
        let summary = config::validate_text(
            r#"
plugins:
  - tag: seq
    type: sequence
    args:
      - matches:
          - qname $domain_rules
        exec: accept
  - tag: domain_rules
    type: domain_set
    args:
      exps:
        - example.com
  - tag: udp_server
    type: udp_server
    args:
      entry: seq
"#,
        )
        .expect("config should validate");

        let graph = render_dependency_graph(&summary);
        assert!(graph.contains("quick_setup(qname) $domain_rules"));
        assert!(graph.contains("deps:"));
        assert!(graph.contains("domain_rules [provider:domain_set]"));
    }

    #[test]
    fn dependency_graph_serializes_sequence_flows_without_dropping_legacy_fields() {
        let summary = config::validate_text(
            r#"
plugins:
  - tag: forward
    type: forward
  - tag: seq
    type: sequence
    args:
      - matches:
          - qname domain:example.com
        exec: $forward
"#,
        )
        .expect("config should validate");

        let value =
            serde_json::to_value(&summary.dependency_graph).expect("graph should serialize");
        assert!(value.get("nodes").is_some());
        assert!(value.get("edges").is_some());
        assert!(value.get("init_order").is_some());

        let flows = value
            .get("sequence_flows")
            .and_then(|flows| flows.as_array())
            .expect("sequence_flows should serialize as an array");
        assert_eq!(flows.len(), 1);
        assert_eq!(
            flows[0].get("tag").and_then(|tag| tag.as_str()),
            Some("seq")
        );
        assert_eq!(
            flows[0]
                .get("rules")
                .and_then(|rules| rules.as_array())
                .and_then(|rules| rules.first())
                .and_then(|rule| rule.get("matches"))
                .and_then(|matches| matches.as_array())
                .and_then(|matches| matches.first())
                .and_then(|expr| expr.get("kind"))
                .and_then(|kind| kind.as_str()),
            Some("quick_setup")
        );
    }

    #[test]
    fn run_check_supports_working_directory_for_relative_paths() {
        let temp = TempDir::new().expect("temp dir");
        write_config(
            temp.path(),
            "config.yaml",
            r#"
plugins:
  - tag: debug_main
    type: debug_print
"#,
        );

        let original_dir = std::env::current_dir().expect("current dir");
        let result = run_check(&CheckOptions {
            config: std::path::PathBuf::from("config.yaml"),
            working_dir: Some(temp.path().to_path_buf()),
            graph: false,
        });
        std::env::set_current_dir(&original_dir).expect("restore current dir");

        assert!(result.is_ok());
    }
}
