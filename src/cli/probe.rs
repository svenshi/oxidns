// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! CLI support for runtime diagnostics.

use std::path::{Path, PathBuf};

use serde_yaml_ng::{Mapping, Value};

use crate::cli::{ProbeCommand, ProbeOptions, ProbeUpstreamOptions};
use crate::config::env_expand;
use crate::config::types::NetworkOutboundConfig;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::outbound;
use crate::infra::network::upstream::UpstreamConfig;
use crate::infra::network::upstream::probe::{
    ProbeProgress, ProbeStageReport, ProbeVerdict, UpstreamProbeConfig, UpstreamProbeReport,
    parse_record_type, probe_upstream, probe_upstream_with_progress,
};

pub fn run(options: ProbeOptions) -> Result<()> {
    match options.command {
        ProbeCommand::Upstream(options) => run_upstream(options),
    }
}

fn run_upstream(options: ProbeUpstreamOptions) -> Result<()> {
    prepare_working_dir(options.working_dir.as_ref())?;
    prepare_outbound(options.config.as_ref())?;

    let qtype = parse_record_type(&options.qtype)?;
    let probe_config = UpstreamProbeConfig {
        upstream: UpstreamConfig {
            tag: Some("cli_probe".to_string()),
            addr: options.addr.clone(),
            outbound: options.outbound.clone(),
            dial_addr: options.dial_addr,
            port: options.port,
            bootstrap: options.bootstrap.clone(),
            bootstrap_version: options.bootstrap_version,
            socks5: options.socks5.clone(),
            idle_timeout: None,
            max_conns: None,
            min_conns: None,
            insecure_skip_verify: Some(options.insecure_skip_verify),
            timeout: Some(options.timeout),
            enable_pipeline: None,
            enable_http3: None,
            so_mark: None,
            bind_to_device: None,
        },
        qname: options.qname.clone(),
        qtype,
        serial_samples: options.serial_samples,
        pipeline_concurrency: options.pipeline_concurrency,
        pipeline_rounds: options.pipeline_rounds,
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| DnsError::runtime(format!("failed to create probe runtime: {err}")))?;
    let report = if options.json {
        runtime.block_on(probe_upstream(probe_config))?
    } else {
        runtime.block_on(probe_upstream_with_progress(probe_config, print_progress))?
    };

    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_report(&report);
    }
    Ok(())
}

fn prepare_working_dir(working_dir: Option<&PathBuf>) -> Result<()> {
    if let Some(working_dir) = working_dir {
        std::env::set_current_dir(working_dir).map_err(|err| {
            DnsError::runtime(format!(
                "failed to switch working directory to {}: {}",
                working_dir.display(),
                err
            ))
        })?;
    }
    Ok(())
}

fn prepare_outbound(config_path: Option<&PathBuf>) -> Result<()> {
    if let Some(config_path) = config_path {
        let outbound_config = read_probe_outbound_config(config_path)?;
        outbound::install_global(&outbound_config)?;
    } else {
        outbound::clear_global();
    }
    Ok(())
}

fn read_probe_outbound_config(config_path: &Path) -> Result<NetworkOutboundConfig> {
    let string = std::fs::read_to_string(config_path).map_err(|err| {
        DnsError::config(format!(
            "failed to read probe config {}: {}",
            config_path.display(),
            err
        ))
    })?;
    let value: Value = serde_yaml_ng::from_str(&string).map_err(|err| {
        DnsError::config(format!(
            "failed to parse probe config {}: {}",
            config_path.display(),
            err
        ))
    })?;
    let mut outbound_value = extract_probe_outbound_value(value).map_err(|err| {
        DnsError::config(format!(
            "failed to read network.outbound from probe config {}: {}",
            config_path.display(),
            err
        ))
    })?;
    env_expand::expand_env_in_value(&mut outbound_value).map_err(|err| {
        DnsError::config(format!(
            "env expansion failed in probe network.outbound config {}: {}",
            config_path.display(),
            err
        ))
    })?;
    serde_yaml_ng::from_value(outbound_value).map_err(|err| {
        DnsError::config(format!(
            "failed to deserialize network.outbound from probe config {}: {}",
            config_path.display(),
            err
        ))
    })
}

fn extract_probe_outbound_value(value: Value) -> std::result::Result<Value, &'static str> {
    let mut root = match value {
        Value::Mapping(root) => root,
        Value::Null => return Ok(empty_mapping_value()),
        _ => return Err("root must be a mapping"),
    };
    let Some(network) = root.remove("network") else {
        return Ok(empty_mapping_value());
    };
    let mut network = match network {
        Value::Mapping(network) => network,
        Value::Null => return Ok(empty_mapping_value()),
        _ => return Err("network must be a mapping"),
    };
    Ok(network
        .remove("outbound")
        .unwrap_or_else(empty_mapping_value))
}

fn empty_mapping_value() -> Value {
    Value::Mapping(Mapping::new())
}

fn print_human_report(report: &UpstreamProbeReport) {
    println!();
    println!("Upstream Probe");
    println!("==============");
    print_kv("Address", report.target.address.as_str());
    print_kv("Protocol", report.target.protocol.as_str());
    print_kv(
        "Server",
        format!("{}:{}", report.target.server_name, report.target.port).as_str(),
    );
    print_kv(
        "Resolved IP",
        report.target.resolved_ip.as_deref().unwrap_or("-"),
    );
    print_kv(
        "Resolution",
        report.target.resolution_source.as_deref().unwrap_or("-"),
    );
    if report.target.uses_bootstrap {
        match report.target.resolution_error.as_deref() {
            Some(error) => print_kv("Bootstrap", format!("failed ({error})").as_str()),
            None => print_kv("Bootstrap", "resolved"),
        }
    }
    print_kv(
        "Query",
        format!("{} {}", report.query.qname, report.query.qtype).as_str(),
    );
    print_kv("Timeout", format!("{}ms", report.timeout_ms).as_str());

    print_serial_report(&report.serial);
    print_pipeline_report(report);
    println!();
    println!("Recommendation");
    println!("--------------");
    println!("{}", report.recommendation);
}

fn print_serial_report(serial: &ProbeStageReport) {
    println!();
    println!("Serial Baseline");
    println!("---------------");
    print_kv("Verdict", verdict_label(serial.verdict));
    print_kv(
        "Success",
        format!("{}/{}", serial.success_count, serial.total_queries).as_str(),
    );
    print_kv(
        "Avg Latency",
        latency_label(serial.average_latency_ms).as_str(),
    );
    print_kv("Failures", serial.failure_count.to_string().as_str());
    if let Some(sample) = serial.results.iter().find(|result| result.ok) {
        print_kv("Rcode", sample.rcode.as_deref().unwrap_or("unknown"));
        print_kv(
            "Answers",
            sample.answer_count.unwrap_or_default().to_string().as_str(),
        );
        print_kv(
            "Truncated",
            sample.truncated.unwrap_or(false).to_string().as_str(),
        );
        print_kv(
            "Recursion",
            sample
                .recursion_available
                .unwrap_or(false)
                .to_string()
                .as_str(),
        );
    }
    print_errors(&serial.errors);
}

fn print_pipeline_report(report: &UpstreamProbeReport) {
    let pipeline = &report.pipeline;
    println!();
    println!(
        "{}",
        if matches!(report.target.protocol.as_str(), "tcp" | "dot") {
            "Pipeline Probe"
        } else {
            "Concurrency Probe"
        }
    );
    println!(
        "{}",
        if matches!(report.target.protocol.as_str(), "tcp" | "dot") {
            "--------------"
        } else {
            "-----------------"
        }
    );
    print_kv("Verdict", verdict_label(pipeline.verdict));
    print_kv("Concurrency", pipeline.concurrency.to_string().as_str());
    print_kv("Rounds", pipeline.rounds.to_string().as_str());
    print_kv(
        "Success",
        format!("{}/{}", pipeline.success_count, pipeline.total_queries).as_str(),
    );
    print_kv("Timeouts", pipeline.timeout_count.to_string().as_str());
    print_kv("Mismatches", pipeline.mismatch_count.to_string().as_str());
    print_kv("Other Errors", pipeline.error_count.to_string().as_str());
    print_kv(
        "Avg Latency",
        latency_label(pipeline.average_latency_ms).as_str(),
    );
    print_errors(&pipeline.errors);
}

fn print_kv(label: &str, value: &str) {
    println!("{label:>14}: {value}");
}

fn print_errors(errors: &[String]) {
    if errors.is_empty() {
        return;
    }
    println!("        Errors:");
    for error in errors.iter().take(5) {
        println!("                - {error}");
    }
}

fn print_progress(event: ProbeProgress) {
    match event {
        ProbeProgress::Preparing { address } => {
            eprintln!("probe: preparing {address}");
        }
        ProbeProgress::Resolved {
            server_name,
            resolved_ip,
            source,
            error,
        } => {
            if let Some(error) = error {
                eprintln!(
                    "probe: resolving {server_name} via {} failed: {error}",
                    source.unwrap_or_else(|| "unknown".to_string())
                );
            } else if let Some(ip) = resolved_ip {
                eprintln!(
                    "probe: resolved {server_name} -> {ip} ({})",
                    source.unwrap_or_else(|| "unknown".to_string())
                );
            } else {
                eprintln!("probe: no pre-resolved IP for {server_name}");
            }
        }
        ProbeProgress::SerialStarted { samples } => {
            eprintln!("probe: running serial baseline ({samples} sample(s))");
        }
        ProbeProgress::SerialSampleFinished { index, ok } => {
            eprintln!(
                "probe: serial sample #{} {}",
                index + 1,
                if ok { "ok" } else { "failed" }
            );
        }
        ProbeProgress::ConcurrencyStarted {
            protocol,
            strategy,
            concurrency,
            rounds,
        } => {
            eprintln!(
                "probe: running {protocol} {strategy} probe ({rounds} round(s), concurrency {concurrency})"
            );
        }
        ProbeProgress::ConcurrencyRoundFinished {
            round,
            success_count,
            total_queries,
        } => {
            eprintln!(
                "probe: concurrency round #{} finished ({success_count}/{total_queries} ok)",
                round + 1
            );
        }
        ProbeProgress::Finished {
            serial,
            concurrency,
        } => {
            eprintln!(
                "probe: finished (serial={}, concurrency={})",
                verdict_label(serial),
                verdict_label(concurrency)
            );
        }
    }
}

fn latency_label(value: Option<u128>) -> String {
    value
        .map(|latency| format!("{latency}ms"))
        .unwrap_or_else(|| "n/a".to_string())
}

fn verdict_label(verdict: ProbeVerdict) -> &'static str {
    match verdict {
        ProbeVerdict::Reachable => "reachable",
        ProbeVerdict::Unreachable => "unreachable",
        ProbeVerdict::Supported => "supported",
        ProbeVerdict::Unsupported => "unsupported",
        ProbeVerdict::Unstable => "unstable",
        ProbeVerdict::Inconclusive => "inconclusive",
        ProbeVerdict::NotApplicable => "not_applicable",
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::infra::network::outbound::TestGlobalGuard;
    use crate::infra::network::upstream::ConnectionInfo;

    #[test]
    fn prepare_outbound_loads_only_network_outbound_from_config() {
        let _guard = TestGlobalGuard::clean();
        let tmp = tempfile::TempDir::new().expect("temp dir should create");
        let config_path = tmp.path().join("config.yaml");
        std::fs::write(
            &config_path,
            r#"
network:
  outbound:
    profiles:
      remote:
        resolver:
          nameservers:
            - addr: 1.1.1.1:53
plugins:
  - tag: ""
    type: ""
    args:
      path: ${OXIDNS_PROBE_REVIEW_UNSET_DO_NOT_DEFINE}
"#,
        )
        .expect("config should write");

        prepare_outbound(Some(&config_path)).expect("outbound-only config should load");

        let info = ConnectionInfo::try_from(UpstreamConfig {
            tag: None,
            addr: "tls://dns.example.invalid:853".to_string(),
            outbound: Some("remote".to_string()),
            dial_addr: None,
            port: None,
            bootstrap: None,
            bootstrap_version: None,
            socks5: None,
            idle_timeout: None,
            max_conns: None,
            min_conns: None,
            insecure_skip_verify: None,
            timeout: Some(Duration::from_secs(1)),
            enable_pipeline: None,
            enable_http3: None,
            so_mark: None,
            bind_to_device: None,
        })
        .expect("outbound resolver should be available to upstream config");

        assert_eq!(
            info.bootstrap
                .as_ref()
                .expect("outbound resolver should be injected")
                .profile(),
            "remote"
        );
    }
}
