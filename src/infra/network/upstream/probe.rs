// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Active diagnostics for configured DNS upstream endpoints.

use std::collections::HashMap;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::dial::{DialTarget, SocketOptions, try_lookup_server_name};
#[cfg(feature = "upstream-dot")]
use crate::infra::network::dial::{TlsDialOptions, connect_tls};
use crate::infra::network::proxy::{Socks5Opt, connect_tcp, parse_socks5_opt};
#[cfg(feature = "upstream-dot")]
use crate::infra::network::transport::tcp::TcpTransport;
use crate::infra::network::transport::tcp::{TcpTransportReader, TcpTransportWriter};
use crate::infra::network::upstream::builder::UpstreamBuilder;
use crate::infra::network::upstream::config::{ConnectionInfo, ConnectionType, UpstreamConfig};
use crate::infra::network::upstream::pool::{DeadlineOutcome, QueryDeadline};
use crate::infra::network::upstream::traits::Upstream;
use crate::proto::{DNSClass, Message, MessageType, Name, Question, RecordType};

const ERROR_KIND_TIMEOUT: &str = "timeout";
const ERROR_KIND_MISMATCH: &str = "mismatch";
const ERROR_KIND_PROTOCOL: &str = "protocol";
const ERROR_KIND_QUERY: &str = "query";
const MAX_PROBE_SAMPLES: usize = 4096;

#[derive(Clone, Debug)]
pub struct UpstreamProbeConfig {
    pub upstream: UpstreamConfig,
    pub qname: String,
    pub qtype: RecordType,
    pub serial_samples: usize,
    pub pipeline_concurrency: usize,
    pub pipeline_rounds: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeVerdict {
    Reachable,
    Unreachable,
    Supported,
    Unsupported,
    Unstable,
    Inconclusive,
    NotApplicable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UpstreamProbeTarget {
    pub address: String,
    pub protocol: String,
    pub server_name: String,
    pub port: u16,
    pub resolved_ip: Option<String>,
    pub resolution_source: Option<String>,
    pub uses_bootstrap: bool,
    pub resolution_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProbeQuery {
    pub qname: String,
    pub qtype: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProbeQueryResult {
    pub index: usize,
    pub query_name: String,
    pub query_id: u16,
    pub ok: bool,
    pub latency_ms: Option<u128>,
    pub response_id: Option<u16>,
    pub rcode: Option<String>,
    pub answer_count: Option<u16>,
    pub authoritative: Option<bool>,
    pub truncated: Option<bool>,
    pub recursion_available: Option<bool>,
    pub error_kind: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProbeStageReport {
    pub verdict: ProbeVerdict,
    pub total_queries: usize,
    pub success_count: usize,
    pub failure_count: usize,
    pub average_latency_ms: Option<u128>,
    pub results: Vec<ProbeQueryResult>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PipelineProbeReport {
    pub verdict: ProbeVerdict,
    pub applicable: bool,
    pub concurrency: usize,
    pub rounds: usize,
    pub total_queries: usize,
    pub success_count: usize,
    pub timeout_count: usize,
    pub mismatch_count: usize,
    pub error_count: usize,
    pub average_latency_ms: Option<u128>,
    pub results: Vec<ProbeQueryResult>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UpstreamProbeReport {
    pub target: UpstreamProbeTarget,
    pub query: ProbeQuery,
    pub timeout_ms: u128,
    pub serial: ProbeStageReport,
    pub pipeline: PipelineProbeReport,
    pub recommendation: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeProgress {
    Preparing {
        address: String,
    },
    Resolved {
        server_name: String,
        resolved_ip: Option<String>,
        source: Option<String>,
        error: Option<String>,
    },
    SerialStarted {
        samples: usize,
    },
    SerialSampleFinished {
        index: usize,
        ok: bool,
    },
    ConcurrencyStarted {
        protocol: String,
        strategy: String,
        concurrency: usize,
        rounds: usize,
    },
    ConcurrencyRoundFinished {
        round: usize,
        success_count: usize,
        total_queries: usize,
    },
    Finished {
        serial: ProbeVerdict,
        concurrency: ProbeVerdict,
    },
}

#[derive(Clone, Debug)]
struct PendingQuery {
    index: usize,
    query_name: String,
    qtype: RecordType,
    sent_at: Instant,
}

pub async fn probe_upstream(config: UpstreamProbeConfig) -> Result<UpstreamProbeReport> {
    probe_upstream_with_progress(config, |_| {}).await
}

pub async fn probe_upstream_with_progress<F>(
    config: UpstreamProbeConfig,
    mut progress: F,
) -> Result<UpstreamProbeReport>
where
    F: FnMut(ProbeProgress),
{
    validate_probe_config(&config)?;
    AppClock::start();
    progress(ProbeProgress::Preparing {
        address: config.upstream.addr.clone(),
    });

    let base_name = parse_name(&config.qname)?;
    let upstream = prepare_probe_upstream_config(config.upstream.clone()).await?;
    let mut connection_info = ConnectionInfo::try_from(upstream.clone())?;
    let uses_bootstrap = connection_info.bootstrap.is_some();
    let resolution = resolve_remote_ip(&connection_info, upstream.dial_addr.is_some()).await;
    if let Some(ip) = resolution.ip
        && resolution.apply_to_connection
    {
        connection_info.remote_ip = Some(ip);
        connection_info.bootstrap = None;
        connection_info.bootstrap_timeout = None;
    }
    progress(ProbeProgress::Resolved {
        server_name: connection_info.server_name.clone(),
        resolved_ip: resolution.ip.map(|ip| ip.to_string()),
        source: resolution.source.clone(),
        error: resolution.error.clone(),
    });
    let block_direct_probe = resolution_blocks_direct_probe(&connection_info, &resolution);

    let mut target = UpstreamProbeTarget {
        address: connection_info.raw_addr.clone(),
        protocol: protocol_name(connection_info.connection_type).to_string(),
        server_name: connection_info.server_name.clone(),
        port: connection_info.port,
        resolved_ip: resolution.ip.map(|ip| ip.to_string()),
        resolution_source: resolution.source.clone(),
        uses_bootstrap,
        resolution_error: resolution.error.clone(),
    };

    let serial = if block_direct_probe {
        run_resolution_failure_serial_probe(
            &config,
            &base_name,
            target
                .resolution_error
                .as_deref()
                .unwrap_or("system resolver failed"),
            &mut progress,
        )
    } else {
        run_serial_probe(&connection_info, &config, &base_name, &mut progress).await?
    };
    let pipeline = run_pipeline_probe(
        &mut connection_info,
        &config,
        serial_baseline_is_clean(&serial),
        &mut progress,
    )
    .await;
    refresh_target_bootstrap_resolution(&mut target, &mut connection_info, &serial, &pipeline)
        .await;
    let recommendation = recommendation(&serial, &pipeline);
    progress(ProbeProgress::Finished {
        serial: serial.verdict,
        concurrency: pipeline.verdict,
    });

    Ok(UpstreamProbeReport {
        target,
        query: ProbeQuery {
            qname: base_name.to_fqdn(),
            qtype: qtype_name(config.qtype),
        },
        timeout_ms: connection_info.timeout.as_millis(),
        serial,
        pipeline,
        recommendation,
    })
}

#[derive(Clone, Debug)]
struct ResolutionProbe {
    ip: Option<IpAddr>,
    source: Option<String>,
    error: Option<String>,
    apply_to_connection: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockingProbeError {
    TimedOut,
    Canceled,
}

fn validate_probe_config(config: &UpstreamProbeConfig) -> Result<()> {
    if config.serial_samples == 0 {
        return Err(DnsError::config(
            "probe serial_samples must be greater than 0",
        ));
    }
    if config.serial_samples > MAX_PROBE_SAMPLES {
        return Err(DnsError::config(format!(
            "probe serial_samples must be <= {MAX_PROBE_SAMPLES}"
        )));
    }
    if config.pipeline_concurrency == 0 {
        return Err(DnsError::config(
            "probe pipeline_concurrency must be greater than 0",
        ));
    }
    if config.pipeline_concurrency > MAX_PROBE_SAMPLES {
        return Err(DnsError::config(format!(
            "probe pipeline_concurrency must be <= {MAX_PROBE_SAMPLES}"
        )));
    }
    if config.pipeline_rounds == 0 {
        return Err(DnsError::config(
            "probe pipeline_rounds must be greater than 0",
        ));
    }
    if config.pipeline_rounds > MAX_PROBE_SAMPLES {
        return Err(DnsError::config(format!(
            "probe pipeline_rounds must be <= {MAX_PROBE_SAMPLES}"
        )));
    }
    if pipeline_sample_count(config)? > MAX_PROBE_SAMPLES {
        return Err(DnsError::config(format!(
            "probe pipeline_concurrency * pipeline_rounds must be <= {MAX_PROBE_SAMPLES}"
        )));
    }
    Ok(())
}

fn pipeline_sample_count(config: &UpstreamProbeConfig) -> Result<usize> {
    config
        .pipeline_concurrency
        .checked_mul(config.pipeline_rounds)
        .ok_or_else(|| DnsError::config("probe pipeline sample count overflowed"))
}

async fn prepare_probe_upstream_config(mut upstream: UpstreamConfig) -> Result<UpstreamConfig> {
    if let Some(socks5) = upstream.socks5.as_deref() {
        let timeout = upstream
            .timeout
            .unwrap_or(ConnectionInfo::DEFAULT_QUERY_TIMEOUT);
        upstream.socks5 = Some(resolve_probe_socks5(socks5, timeout).await?);
    }
    Ok(upstream)
}

async fn resolve_probe_socks5(raw: &str, timeout: Duration) -> Result<String> {
    resolve_probe_socks5_with(raw, timeout, |raw| parse_socks5_opt(raw.as_str())).await
}

async fn resolve_probe_socks5_with<F>(raw: &str, timeout: Duration, parse: F) -> Result<String>
where
    F: FnOnce(String) -> Option<Socks5Opt> + Send + 'static,
{
    let raw_owned = raw.to_string();
    match run_probe_blocking_with_timeout(timeout, move || parse(raw_owned)).await {
        Ok(Some(socks5)) => Ok(socks5.to_resolved_config_string()),
        Ok(None) => Err(DnsError::plugin(format!(
            "upstream has invalid socks5 proxy '{raw}'"
        ))),
        Err(BlockingProbeError::Canceled) => Err(DnsError::plugin(
            "SOCKS5 proxy resolver task was canceled".to_string(),
        )),
        Err(BlockingProbeError::TimedOut) => Err(DnsError::plugin(format!(
            "SOCKS5 proxy resolution timed out after {timeout:?}"
        ))),
    }
}

async fn run_probe_blocking_with_timeout<T, F>(
    timeout: Duration,
    operation: F,
) -> std::result::Result<T, BlockingProbeError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = sender.send(operation());
    });
    match tokio::time::timeout(timeout, receiver).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => Err(BlockingProbeError::Canceled),
        Err(_) => Err(BlockingProbeError::TimedOut),
    }
}

async fn resolve_remote_ip(info: &ConnectionInfo, has_dial_addr: bool) -> ResolutionProbe {
    if let Some(ip) = info.remote_ip {
        let source = if has_dial_addr {
            "dial_addr"
        } else if IpAddr::from_str(&info.server_name).is_ok() {
            "literal"
        } else {
            "configured"
        };
        return ResolutionProbe {
            ip: Some(ip),
            source: Some(source.to_string()),
            error: None,
            apply_to_connection: true,
        };
    }

    if let Some(resolver) = info.bootstrap.as_ref() {
        let timeout = info.bootstrap_timeout.unwrap_or(info.timeout);
        return match resolver
            .resolve(&info.server_name, QueryDeadline::new(timeout))
            .await
        {
            Ok(ip) => ResolutionProbe {
                ip: Some(ip),
                source: Some("bootstrap".to_string()),
                error: None,
                apply_to_connection: true,
            },
            Err(err) => ResolutionProbe {
                ip: None,
                source: Some("bootstrap".to_string()),
                error: Some(err.to_string()),
                apply_to_connection: false,
            },
        };
    }

    if should_delegate_name_resolution_to_socks5(info) {
        return ResolutionProbe {
            ip: None,
            source: Some("proxy".to_string()),
            error: None,
            apply_to_connection: false,
        };
    }

    let server_name = info.server_name.clone();
    match run_probe_blocking_with_timeout(info.timeout, move || {
        try_lookup_server_name(&server_name)
    })
    .await
    {
        Ok(Ok(ip)) => ResolutionProbe {
            ip: Some(ip),
            source: Some("system".to_string()),
            error: None,
            apply_to_connection: info.socks5.is_none(),
        },
        Ok(Err(err)) => ResolutionProbe {
            ip: None,
            source: Some("system".to_string()),
            error: Some(err.to_string()),
            apply_to_connection: false,
        },
        Err(BlockingProbeError::Canceled) => ResolutionProbe {
            ip: None,
            source: Some("system".to_string()),
            error: Some("system resolver task was canceled".to_string()),
            apply_to_connection: false,
        },
        Err(BlockingProbeError::TimedOut) => ResolutionProbe {
            ip: None,
            source: Some("system".to_string()),
            error: Some(format!(
                "system resolver timed out after {:?}",
                info.timeout
            )),
            apply_to_connection: false,
        },
    }
}

fn should_delegate_name_resolution_to_socks5(info: &ConnectionInfo) -> bool {
    // Match upstream dialing: SOCKS5 receives the domain only when no configured
    // resolver or static remote IP can provide a concrete connection address.
    info.remote_ip.is_none() && info.bootstrap.is_none() && info.socks5.is_some()
}

fn resolution_blocks_direct_probe(info: &ConnectionInfo, resolution: &ResolutionProbe) -> bool {
    resolution.ip.is_none()
        && resolution.error.is_some()
        && resolution.source.as_deref() == Some("system")
        && info.remote_ip.is_none()
        && info.bootstrap.is_none()
        && info.socks5.is_none()
}

async fn refresh_target_bootstrap_resolution(
    target: &mut UpstreamProbeTarget,
    connection_info: &mut ConnectionInfo,
    serial: &ProbeStageReport,
    pipeline: &PipelineProbeReport,
) {
    if !target.uses_bootstrap || target.resolved_ip.is_some() {
        return;
    }
    if serial.success_count == 0 && pipeline.success_count == 0 {
        return;
    }
    if connection_info.remote_ip.is_none() && connection_info.bootstrap.is_some() {
        let _ = resolve_bootstrap_remote_ip(connection_info).await;
    }
    refresh_target_from_connection_remote_ip(target, connection_info);
}

fn refresh_target_from_connection_remote_ip(
    target: &mut UpstreamProbeTarget,
    connection_info: &ConnectionInfo,
) {
    if let Some(remote_ip) = connection_info.remote_ip {
        target.resolved_ip = Some(remote_ip.to_string());
        target.resolution_source = Some("bootstrap".to_string());
        target.resolution_error = None;
    }
}

async fn run_serial_probe<F>(
    connection_info: &ConnectionInfo,
    config: &UpstreamProbeConfig,
    base_name: &Name,
    progress: &mut F,
) -> Result<ProbeStageReport>
where
    F: FnMut(ProbeProgress),
{
    progress(ProbeProgress::SerialStarted {
        samples: config.serial_samples,
    });
    let upstream = UpstreamBuilder::with_connection_info(connection_info.clone())?;
    let mut results = Vec::with_capacity(config.serial_samples);

    for index in 0..config.serial_samples {
        let query_id = probe_query_id(0, index);
        let request = make_query(query_id, base_name.clone(), config.qtype);
        let started = Instant::now();
        let result = match upstream.query(request).await {
            Ok(response) => result_from_response(
                index,
                query_id,
                base_name.to_fqdn(),
                config.qtype,
                started,
                response,
            ),
            Err(err) => query_error_result(
                index,
                query_id,
                base_name.to_fqdn(),
                ERROR_KIND_QUERY,
                err.to_string(),
                Some(started.elapsed().as_millis()),
            ),
        };
        progress(ProbeProgress::SerialSampleFinished {
            index,
            ok: result.ok,
        });
        results.push(result);
    }

    let success_count = results.iter().filter(|result| result.ok).count();
    let verdict = if success_count == 0 {
        ProbeVerdict::Unreachable
    } else {
        ProbeVerdict::Reachable
    };

    Ok(ProbeStageReport::from_results(verdict, results))
}

fn run_resolution_failure_serial_probe<F>(
    config: &UpstreamProbeConfig,
    base_name: &Name,
    error: &str,
    progress: &mut F,
) -> ProbeStageReport
where
    F: FnMut(ProbeProgress),
{
    progress(ProbeProgress::SerialStarted {
        samples: config.serial_samples,
    });
    let mut results = Vec::with_capacity(config.serial_samples);
    for index in 0..config.serial_samples {
        let result = query_error_result(
            index,
            probe_query_id(0, index),
            base_name.to_fqdn(),
            query_error_kind(error),
            error.to_string(),
            None,
        );
        progress(ProbeProgress::SerialSampleFinished { index, ok: false });
        results.push(result);
    }
    ProbeStageReport::from_results(ProbeVerdict::Unreachable, results)
}

fn serial_baseline_is_clean(serial: &ProbeStageReport) -> bool {
    serial.total_queries > 0 && serial.success_count == serial.total_queries
}

async fn run_pipeline_probe<F>(
    connection_info: &mut ConnectionInfo,
    config: &UpstreamProbeConfig,
    serial_baseline_clean: bool,
    progress: &mut F,
) -> PipelineProbeReport
where
    F: FnMut(ProbeProgress),
{
    let strategy = if uses_single_connection_pipeline(connection_info.connection_type) {
        "single_connection_pipeline"
    } else {
        "concurrent_upstream_queries"
    };
    progress(ProbeProgress::ConcurrencyStarted {
        protocol: protocol_name(connection_info.connection_type).to_string(),
        strategy: strategy.to_string(),
        concurrency: config.pipeline_concurrency,
        rounds: config.pipeline_rounds,
    });

    if !serial_baseline_clean {
        return PipelineProbeReport::inconclusive(
            config.pipeline_concurrency,
            config.pipeline_rounds,
            "serial baseline was not fully successful; concurrency behavior cannot be isolated"
                .to_string(),
        );
    }

    if uses_single_connection_pipeline(connection_info.connection_type)
        && connection_info.remote_ip.is_none()
        && connection_info.bootstrap.is_some()
        && let Err(err) = resolve_bootstrap_remote_ip(connection_info).await
    {
        return PipelineProbeReport::inconclusive(
            config.pipeline_concurrency,
            config.pipeline_rounds,
            format!("bootstrap resolution failed before pipeline probe: {err}"),
        );
    }

    let capacity = pipeline_sample_count(config).unwrap_or(MAX_PROBE_SAMPLES);
    let mut results = Vec::with_capacity(capacity);
    let mut stage_errors = Vec::new();
    if uses_single_connection_pipeline(connection_info.connection_type) {
        for round in 0..config.pipeline_rounds {
            match run_pipeline_round(connection_info, config, round).await {
                Ok(mut round_results) => {
                    let success_count = round_results.iter().filter(|result| result.ok).count();
                    let total_queries = round_results.len();
                    progress(ProbeProgress::ConcurrencyRoundFinished {
                        round,
                        success_count,
                        total_queries,
                    });
                    results.append(&mut round_results);
                }
                Err(err) => {
                    stage_errors.push(err.to_string());
                    let round_results = connection_error_results(config, round, err.to_string());
                    progress(ProbeProgress::ConcurrencyRoundFinished {
                        round,
                        success_count: 0,
                        total_queries: round_results.len(),
                    });
                    results.extend(round_results);
                }
            }
        }
    } else {
        match run_generic_concurrency_probe(connection_info, config, progress).await {
            Ok(mut generic_results) => results.append(&mut generic_results),
            Err(err) => {
                stage_errors.push(err.to_string());
                for round in 0..config.pipeline_rounds {
                    let round_results = connection_error_results(config, round, err.to_string());
                    progress(ProbeProgress::ConcurrencyRoundFinished {
                        round,
                        success_count: 0,
                        total_queries: round_results.len(),
                    });
                    results.extend(round_results);
                }
            }
        }
    }

    PipelineProbeReport::from_results(
        true,
        config.pipeline_concurrency,
        config.pipeline_rounds,
        results,
        stage_errors,
    )
}

fn uses_single_connection_pipeline(connection_type: ConnectionType) -> bool {
    matches!(connection_type, ConnectionType::TCP | ConnectionType::DoT)
}

async fn resolve_bootstrap_remote_ip(connection_info: &mut ConnectionInfo) -> Result<()> {
    let Some(resolver) = connection_info.bootstrap.as_ref() else {
        return Ok(());
    };
    let timeout = connection_info
        .bootstrap_timeout
        .unwrap_or(connection_info.timeout);
    let ip = resolver
        .resolve(&connection_info.server_name, QueryDeadline::new(timeout))
        .await?;
    connection_info.remote_ip = Some(ip);
    connection_info.bootstrap = None;
    connection_info.bootstrap_timeout = None;
    Ok(())
}

async fn run_generic_concurrency_probe<F>(
    connection_info: &ConnectionInfo,
    config: &UpstreamProbeConfig,
    progress: &mut F,
) -> Result<Vec<ProbeQueryResult>>
where
    F: FnMut(ProbeProgress),
{
    let upstream: Arc<dyn Upstream> = Arc::from(UpstreamBuilder::with_connection_info(
        connection_info.clone(),
    )?);
    let capacity = pipeline_sample_count(config).unwrap_or(MAX_PROBE_SAMPLES);
    let mut results = Vec::with_capacity(capacity);

    for round in 0..config.pipeline_rounds {
        let mut futures = FuturesUnordered::new();
        for index in 0..config.pipeline_concurrency {
            let upstream = upstream.clone();
            let global_index = round * config.pipeline_concurrency + index;
            let query_id = probe_query_id(round + 1, index);
            let query_name = pipeline_name(&config.qname, round, index)?;
            let query_name_text = query_name.to_fqdn();
            let request = make_query(query_id, query_name, config.qtype);
            let qtype = config.qtype;
            let sent_at = Instant::now();
            futures.push(async move {
                match upstream.query(request).await {
                    Ok(response) => result_from_response(
                        global_index,
                        query_id,
                        query_name_text,
                        qtype,
                        sent_at,
                        response,
                    ),
                    Err(err) => {
                        let error = err.to_string();
                        query_error_result(
                            global_index,
                            query_id,
                            query_name_text,
                            query_error_kind(error.as_str()),
                            error,
                            Some(sent_at.elapsed().as_millis()),
                        )
                    }
                }
            });
        }

        while let Some(result) = futures.next().await {
            results.push(result);
        }
        let round_start = round * config.pipeline_concurrency;
        let round_end = round_start + config.pipeline_concurrency;
        let success_count = results
            .iter()
            .filter(|result| result.index >= round_start && result.index < round_end && result.ok)
            .count();
        progress(ProbeProgress::ConcurrencyRoundFinished {
            round,
            success_count,
            total_queries: config.pipeline_concurrency,
        });
    }

    results.sort_by_key(|result| result.index);
    Ok(results)
}

async fn run_pipeline_round(
    connection_info: &ConnectionInfo,
    config: &UpstreamProbeConfig,
    round: usize,
) -> Result<Vec<ProbeQueryResult>> {
    let deadline = QueryDeadline::new(connection_info.timeout);
    let stream = match deadline
        .run(connect_tcp(
            DialTarget::new(
                connection_info.remote_ip,
                connection_info.server_name.clone(),
                connection_info.port,
            ),
            SocketOptions::new(
                connection_info.so_mark,
                connection_info.bind_to_device.clone(),
            ),
            connection_info.socks5.clone(),
        ))
        .await
    {
        DeadlineOutcome::Completed(result) => result?,
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    };

    match connection_info.connection_type {
        ConnectionType::TCP => {
            let (reader, writer) = stream.into_split();
            run_pipeline_round_with_io(
                TcpTransportReader::new(reader),
                TcpTransportWriter::new(writer),
                config,
                round,
                &deadline,
            )
            .await
        }
        ConnectionType::DoT => {
            run_dot_pipeline_round(stream, connection_info, config, round, &deadline).await
        }
        _ => Err(DnsError::protocol(
            "pipeline probe reached non-TCP upstream",
        )),
    }
}

#[cfg(feature = "upstream-dot")]
async fn run_dot_pipeline_round(
    stream: tokio::net::TcpStream,
    connection_info: &ConnectionInfo,
    config: &UpstreamProbeConfig,
    round: usize,
    deadline: &QueryDeadline,
) -> Result<Vec<ProbeQueryResult>> {
    let tls_stream = connect_tls(
        stream,
        TlsDialOptions::new(
            DialTarget::new(
                connection_info.remote_ip,
                connection_info.server_name.clone(),
                connection_info.port,
            ),
            connection_info.insecure_skip_verify,
            deadline
                .remaining()
                .ok_or_else(|| deadline.timeout_error())?,
            vec![b"dot".to_vec()],
        ),
    )
    .await?;
    let transport = TcpTransport::new(tls_stream);
    let (reader, writer) = transport.into_split();
    run_pipeline_round_with_io(reader, writer, config, round, deadline).await
}

#[cfg(not(feature = "upstream-dot"))]
async fn run_dot_pipeline_round(
    _stream: tokio::net::TcpStream,
    _connection_info: &ConnectionInfo,
    _config: &UpstreamProbeConfig,
    _round: usize,
    _deadline: &QueryDeadline,
) -> Result<Vec<ProbeQueryResult>> {
    Err(DnsError::plugin(
        "upstream DoT is not compiled into this build; rebuild with --features upstream-dot",
    ))
}

async fn run_pipeline_round_with_io<R, W>(
    mut reader: TcpTransportReader<R>,
    mut writer: TcpTransportWriter<W>,
    config: &UpstreamProbeConfig,
    round: usize,
    deadline: &QueryDeadline,
) -> Result<Vec<ProbeQueryResult>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut pending = HashMap::with_capacity(config.pipeline_concurrency);
    let mut results = Vec::with_capacity(config.pipeline_concurrency);
    let mut unexpected_count = 0usize;
    let max_unexpected = config.pipeline_concurrency;

    for index in 0..config.pipeline_concurrency {
        let global_index = round * config.pipeline_concurrency + index;
        let query_id = probe_query_id(round + 1, index);
        let query_name = pipeline_name(&config.qname, round, index)?;
        let request = make_query(query_id, query_name.clone(), config.qtype);
        match deadline.run(writer.write_message(&request)).await {
            DeadlineOutcome::Completed(result) => result?,
            DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
        }
        pending.insert(
            query_id,
            PendingQuery {
                index: global_index,
                query_name: query_name.to_fqdn(),
                qtype: config.qtype,
                sent_at: Instant::now(),
            },
        );
    }

    while !pending.is_empty() {
        let Some(remaining) = deadline.remaining() else {
            break;
        };
        match tokio::time::timeout(remaining, reader.read_message()).await {
            Ok(Ok(response)) => {
                let response_id = response.id();
                let Some(pending_query) = pending.remove(&response_id) else {
                    unexpected_count += 1;
                    if unexpected_count <= max_unexpected {
                        results.push(unexpected_response_result(response));
                    }
                    if unexpected_count >= max_unexpected {
                        results.extend(pending.drain().map(|(query_id, pending_query)| {
                            query_error_result(
                                pending_query.index,
                                query_id,
                                pending_query.query_name,
                                ERROR_KIND_PROTOCOL,
                                "too many unexpected pipeline responses".to_string(),
                                Some(pending_query.sent_at.elapsed().as_millis()),
                            )
                        }));
                        results.sort_by_key(|result| result.index);
                        return Ok(results);
                    }
                    continue;
                };
                results.push(result_from_response(
                    pending_query.index,
                    response_id,
                    pending_query.query_name,
                    pending_query.qtype,
                    pending_query.sent_at,
                    response,
                ));
            }
            Ok(Err(err)) => {
                results.extend(pending.drain().map(|(query_id, pending_query)| {
                    query_error_result(
                        pending_query.index,
                        query_id,
                        pending_query.query_name,
                        ERROR_KIND_PROTOCOL,
                        format!("connection closed before response: {err}"),
                        Some(pending_query.sent_at.elapsed().as_millis()),
                    )
                }));
                return Ok(results);
            }
            Err(_) => break,
        }
    }

    results.extend(pending.drain().map(|(query_id, pending_query)| {
        query_error_result(
            pending_query.index,
            query_id,
            pending_query.query_name,
            ERROR_KIND_TIMEOUT,
            "timed out waiting for pipelined response".to_string(),
            Some(pending_query.sent_at.elapsed().as_millis()),
        )
    }));
    results.sort_by_key(|result| result.index);
    Ok(results)
}

fn connection_error_results(
    config: &UpstreamProbeConfig,
    round: usize,
    error: String,
) -> Vec<ProbeQueryResult> {
    let error_kind = connection_error_kind(error.as_str());
    (0..config.pipeline_concurrency)
        .map(|index| {
            let global_index = round * config.pipeline_concurrency + index;
            let query_id = probe_query_id(round + 1, index);
            let query_name = pipeline_name(&config.qname, round, index)
                .map(|name| name.to_fqdn())
                .unwrap_or_else(|_| config.qname.clone());
            query_error_result(
                global_index,
                query_id,
                query_name,
                error_kind,
                error.clone(),
                None,
            )
        })
        .collect()
}

impl ProbeStageReport {
    fn from_results(verdict: ProbeVerdict, results: Vec<ProbeQueryResult>) -> Self {
        let total_queries = results.len();
        let success_count = results.iter().filter(|result| result.ok).count();
        let failure_count = total_queries.saturating_sub(success_count);
        let average_latency_ms = average_latency(&results);
        let errors = collect_errors(&results, Vec::new());
        Self {
            verdict,
            total_queries,
            success_count,
            failure_count,
            average_latency_ms,
            results,
            errors,
        }
    }
}

impl PipelineProbeReport {
    fn inconclusive(concurrency: usize, rounds: usize, reason: String) -> Self {
        Self {
            verdict: ProbeVerdict::Inconclusive,
            applicable: true,
            concurrency,
            rounds,
            total_queries: 0,
            success_count: 0,
            timeout_count: 0,
            mismatch_count: 0,
            error_count: 0,
            average_latency_ms: None,
            results: Vec::new(),
            errors: vec![reason],
        }
    }

    fn from_results(
        applicable: bool,
        concurrency: usize,
        rounds: usize,
        results: Vec<ProbeQueryResult>,
        stage_errors: Vec<String>,
    ) -> Self {
        let total_queries = results.len();
        let success_count = results.iter().filter(|result| result.ok).count();
        let timeout_count = count_error_kind(&results, ERROR_KIND_TIMEOUT);
        let mismatch_count = count_error_kind(&results, ERROR_KIND_MISMATCH);
        let error_count = results
            .iter()
            .filter(|result| {
                !result.ok
                    && !matches!(
                        result.error_kind.as_deref(),
                        Some(ERROR_KIND_TIMEOUT | ERROR_KIND_MISMATCH)
                    )
            })
            .count();
        let verdict = pipeline_verdict(total_queries, success_count, timeout_count, mismatch_count);
        let average_latency_ms = average_latency(&results);
        let errors = collect_errors(&results, stage_errors);

        Self {
            verdict,
            applicable,
            concurrency,
            rounds,
            total_queries,
            success_count,
            timeout_count,
            mismatch_count,
            error_count,
            average_latency_ms,
            results,
            errors,
        }
    }
}

fn pipeline_verdict(
    total_queries: usize,
    success_count: usize,
    timeout_count: usize,
    mismatch_count: usize,
) -> ProbeVerdict {
    if total_queries == 0 {
        ProbeVerdict::Inconclusive
    } else if success_count == total_queries {
        ProbeVerdict::Supported
    } else if mismatch_count > 0 {
        ProbeVerdict::Unstable
    } else if success_count == 0 && timeout_count > 0 {
        ProbeVerdict::Unsupported
    } else {
        ProbeVerdict::Unstable
    }
}

fn average_latency(results: &[ProbeQueryResult]) -> Option<u128> {
    let mut count = 0u128;
    let mut total = 0u128;
    for latency in results.iter().filter_map(|result| result.latency_ms) {
        total = total.saturating_add(latency);
        count += 1;
    }
    (count > 0).then_some(total / count)
}

fn count_error_kind(results: &[ProbeQueryResult], kind: &str) -> usize {
    results
        .iter()
        .filter(|result| result.error_kind.as_deref() == Some(kind))
        .count()
}

fn collect_errors(results: &[ProbeQueryResult], mut stage_errors: Vec<String>) -> Vec<String> {
    for error in results.iter().filter_map(|result| result.error.as_ref()) {
        if !stage_errors.iter().any(|existing| existing == error) {
            stage_errors.push(error.clone());
        }
        if stage_errors.len() >= 8 {
            break;
        }
    }
    stage_errors
}

fn result_from_response(
    index: usize,
    query_id: u16,
    query_name: String,
    qtype: RecordType,
    started: Instant,
    response: Message,
) -> ProbeQueryResult {
    let response_id = response.id();
    let validation_error = validate_response(&response, query_id, query_name.as_str(), qtype);
    let ok = validation_error.is_none();
    ProbeQueryResult {
        index,
        query_name,
        query_id,
        ok,
        latency_ms: Some(started.elapsed().as_millis()),
        response_id: Some(response_id),
        rcode: Some(format!("{:?}", response.rcode())),
        answer_count: Some(response.answer_count()),
        authoritative: Some(response.authoritative()),
        truncated: Some(response.truncated()),
        recursion_available: Some(response.recursion_available()),
        error_kind: validation_error
            .as_ref()
            .map(|_| ERROR_KIND_MISMATCH.to_string()),
        error: validation_error,
    }
}

fn validate_response(
    response: &Message,
    query_id: u16,
    query_name: &str,
    qtype: RecordType,
) -> Option<String> {
    if response.id() != query_id {
        return Some(format!(
            "response ID mismatch: expected {}, got {}",
            query_id,
            response.id()
        ));
    }
    if response.message_type() != MessageType::Response {
        return Some("message is not a DNS response".to_string());
    }
    let Some(question) = response.first_question() else {
        return Some("response does not echo a question".to_string());
    };
    let response_name = question.name().to_fqdn();
    if !response_name.eq_ignore_ascii_case(query_name) {
        return Some(format!(
            "response question mismatch: expected {}, got {}",
            query_name, response_name
        ));
    }
    if question.qtype() != qtype {
        return Some(format!(
            "response qtype mismatch: expected {}, got {}",
            qtype_name(qtype),
            qtype_name(question.qtype())
        ));
    }
    None
}

fn query_error_result(
    index: usize,
    query_id: u16,
    query_name: String,
    error_kind: &str,
    error: String,
    latency_ms: Option<u128>,
) -> ProbeQueryResult {
    ProbeQueryResult {
        index,
        query_name,
        query_id,
        ok: false,
        latency_ms,
        response_id: None,
        rcode: None,
        answer_count: None,
        authoritative: None,
        truncated: None,
        recursion_available: None,
        error_kind: Some(error_kind.to_string()),
        error: Some(error),
    }
}

fn query_error_kind(error: &str) -> &'static str {
    if is_timeout_error(error) {
        ERROR_KIND_TIMEOUT
    } else {
        ERROR_KIND_QUERY
    }
}

fn connection_error_kind(error: &str) -> &'static str {
    if is_timeout_error(error) {
        ERROR_KIND_TIMEOUT
    } else {
        ERROR_KIND_PROTOCOL
    }
}

fn is_timeout_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("timeout") || error.contains("timed out")
}

fn unexpected_response_result(response: Message) -> ProbeQueryResult {
    ProbeQueryResult {
        index: usize::MAX,
        query_name: "<unexpected>".to_string(),
        query_id: response.id(),
        ok: false,
        latency_ms: None,
        response_id: Some(response.id()),
        rcode: Some(format!("{:?}", response.rcode())),
        answer_count: Some(response.answer_count()),
        authoritative: Some(response.authoritative()),
        truncated: Some(response.truncated()),
        recursion_available: Some(response.recursion_available()),
        error_kind: Some(ERROR_KIND_MISMATCH.to_string()),
        error: Some(format!("unexpected response ID {}", response.id())),
    }
}

fn make_query(id: u16, name: Name, qtype: RecordType) -> Message {
    let mut request = Message::new();
    request.set_id(id);
    request.set_recursion_desired(true);
    request.add_question(Question::new(name, qtype, DNSClass::IN));
    request
}

fn parse_name(raw: &str) -> Result<Name> {
    Name::from_ascii(raw)
        .map_err(|err| DnsError::protocol(format!("invalid probe qname '{}': {}", raw, err)))
}

fn pipeline_name(base: &str, round: usize, index: usize) -> Result<Name> {
    let base = base.trim_end_matches('.');
    let fallback = if base.is_empty() {
        None
    } else {
        Some(parse_name(format!("{base}.").as_str())?)
    };
    let raw = if base.is_empty() {
        format!("oxidns-probe-{round}-{index}.")
    } else {
        format!("oxidns-probe-{round}-{index}.{base}.")
    };
    match parse_name(&raw) {
        Ok(name) => Ok(name),
        Err(_) => fallback
            .ok_or_else(|| DnsError::protocol(format!("invalid probe pipeline qname '{}'", raw))),
    }
}

fn probe_query_id(round: usize, index: usize) -> u16 {
    0x4000u16.wrapping_add((round as u16).wrapping_mul(257)) ^ index as u16
}

fn qtype_name(qtype: RecordType) -> String {
    <&'static str>::from(qtype).to_string()
}

fn protocol_name(connection_type: ConnectionType) -> &'static str {
    match connection_type {
        ConnectionType::UDP => "udp",
        ConnectionType::TCP => "tcp",
        ConnectionType::DoT => "dot",
        ConnectionType::DoQ => "doq",
        ConnectionType::DoH => "doh",
    }
}

pub fn parse_record_type(raw: &str) -> Result<RecordType> {
    RecordType::from_str(raw.trim().to_ascii_uppercase().as_str())
        .map_err(|err| DnsError::config(format!("invalid probe qtype '{}': {}", raw, err)))
}

fn recommendation(serial: &ProbeStageReport, pipeline: &PipelineProbeReport) -> String {
    if serial.verdict == ProbeVerdict::Unreachable {
        return "serial DNS queries failed; check the address, protocol, bootstrap, proxy, firewall, or timeout".to_string();
    }

    match pipeline.verdict {
        ProbeVerdict::Supported => {
            "concurrent querying appears safe for this upstream at the tested concurrency"
                .to_string()
        }
        ProbeVerdict::Unsupported => {
            "avoid enabling pipeline or high concurrency for this upstream; concurrent queries did not complete".to_string()
        }
        ProbeVerdict::Unstable => {
            "avoid enabling pipeline or high concurrency for this upstream; responses were incomplete, mismatched, or inconsistent".to_string()
        }
        ProbeVerdict::Inconclusive => {
            "pipeline behavior is inconclusive; retry with a larger timeout or lower concurrency".to_string()
        }
        ProbeVerdict::NotApplicable => {
            "pipeline probing is not applicable to this protocol".to_string()
        }
        _ => "review the probe results before changing upstream settings".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use tokio::net::{TcpListener, TcpStream, UdpSocket};

    use super::*;
    use crate::infra::network::resolver::NameResolver;
    use crate::proto::rdata::A;
    use crate::proto::{RData, Rcode, Record};

    #[derive(Clone, Copy)]
    enum FakeBehavior {
        Reverse,
        DropPipelined,
        SwapQuestions,
        UnexpectedFlood,
    }

    fn make_upstream_config(addr: String, timeout: Duration) -> UpstreamConfig {
        UpstreamConfig {
            tag: None,
            addr,
            outbound: None,
            dial_addr: None,
            port: None,
            bootstrap: None,
            bootstrap_version: None,
            socks5: None,
            idle_timeout: None,
            max_conns: None,
            min_conns: None,
            insecure_skip_verify: None,
            timeout: Some(timeout),
            enable_pipeline: None,
            enable_http3: None,
            so_mark: None,
            bind_to_device: None,
        }
    }

    async fn start_fake_tcp_server(behavior: FakeBehavior) -> SocketAddr {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("listener should have addr");
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(handle_fake_tcp_client(stream, behavior));
            }
        });
        addr
    }

    async fn start_fake_udp_server() -> SocketAddr {
        let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("UDP socket should bind");
        let addr = socket.local_addr().expect("UDP socket should have addr");
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                let Ok((len, peer)) = socket.recv_from(&mut buf).await else {
                    break;
                };
                let Ok(request) = Message::from_bytes(&buf[..len]) else {
                    continue;
                };
                let response = response_for_request(request);
                let Ok(bytes) = response.to_bytes() else {
                    continue;
                };
                let _ = socket.send_to(&bytes, peer).await;
            }
        });
        addr
    }

    async fn start_fake_bootstrap_resolver(answer_ip: Ipv4Addr) -> SocketAddr {
        let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("bootstrap resolver socket should bind");
        let addr = socket
            .local_addr()
            .expect("bootstrap resolver should have addr");
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                let Ok((len, peer)) = socket.recv_from(&mut buf).await else {
                    break;
                };
                let Ok(request) = Message::from_bytes(&buf[..len]) else {
                    continue;
                };
                let response = resolver_response_for_request(request, answer_ip);
                let Ok(bytes) = response.to_bytes() else {
                    continue;
                };
                let _ = socket.send_to(&bytes, peer).await;
            }
        });
        addr
    }

    async fn handle_fake_tcp_client(stream: TcpStream, behavior: FakeBehavior) {
        let (reader, writer) = stream.into_split();
        let mut reader = TcpTransportReader::new(reader);
        let mut writer = TcpTransportWriter::new(writer);

        loop {
            let Ok(first) = reader.read_message().await else {
                break;
            };
            let mut batch = vec![first];
            while batch.len() < 32 {
                match tokio::time::timeout(Duration::from_millis(15), reader.read_message()).await {
                    Ok(Ok(message)) => batch.push(message),
                    _ => break,
                }
            }

            if batch.len() > 1 && matches!(behavior, FakeBehavior::DropPipelined) {
                continue;
            }

            let responses = fake_responses(batch, behavior);
            for response in responses {
                if writer.write_message(&response).await.is_err() {
                    return;
                }
            }
        }
    }

    fn fake_responses(mut requests: Vec<Message>, behavior: FakeBehavior) -> Vec<Message> {
        match behavior {
            FakeBehavior::Reverse => {
                requests.reverse();
                requests.into_iter().map(response_for_request).collect()
            }
            FakeBehavior::DropPipelined => requests.into_iter().map(response_for_request).collect(),
            FakeBehavior::UnexpectedFlood if requests.len() > 1 => {
                let question = requests
                    .first()
                    .and_then(Message::first_question)
                    .expect("request should have question")
                    .clone();
                (0..requests.len().saturating_mul(4))
                    .map(|index| response_with_question(0x7000 + index as u16, question.clone()))
                    .collect()
            }
            FakeBehavior::UnexpectedFlood => {
                requests.into_iter().map(response_for_request).collect()
            }
            FakeBehavior::SwapQuestions if requests.len() > 1 => {
                let questions = requests
                    .iter()
                    .map(|request| {
                        request
                            .first_question()
                            .expect("request should have question")
                            .clone()
                    })
                    .collect::<Vec<_>>();
                requests
                    .into_iter()
                    .enumerate()
                    .map(|(idx, request)| {
                        let swapped = questions[(idx + 1) % questions.len()].clone();
                        response_with_question(request.id(), swapped)
                    })
                    .collect()
            }
            FakeBehavior::SwapQuestions => requests.into_iter().map(response_for_request).collect(),
        }
    }

    fn response_for_request(request: Message) -> Message {
        let question = request
            .first_question()
            .expect("request should have question")
            .clone();
        response_with_question(request.id(), question)
    }

    fn resolver_response_for_request(request: Message, answer_ip: Ipv4Addr) -> Message {
        let question = request
            .first_question()
            .expect("resolver request should have question")
            .clone();
        let mut response = response_with_question(request.id(), question.clone());
        response.add_answer(Record::from_rdata(
            question.name().clone(),
            60,
            RData::A(A(answer_ip)),
        ));
        response
    }

    fn response_with_question(id: u16, question: Question) -> Message {
        let mut response = Message::new();
        response.set_id(id);
        response.set_message_type(MessageType::Response);
        response.set_recursion_desired(true);
        response.set_recursion_available(true);
        response.set_rcode(Rcode::NoError);
        response.add_question(question);
        response
    }

    async fn probe_fake_server(
        addr: SocketAddr,
        behavior_timeout: Duration,
    ) -> UpstreamProbeReport {
        probe_upstream(UpstreamProbeConfig {
            upstream: make_upstream_config(format!("tcp://{addr}"), behavior_timeout),
            qname: "example.com.".to_string(),
            qtype: RecordType::A,
            serial_samples: 1,
            pipeline_concurrency: 4,
            pipeline_rounds: 1,
        })
        .await
        .expect("probe should run")
    }

    #[tokio::test]
    async fn probe_pipeline_supported_with_out_of_order_responses() {
        let addr = start_fake_tcp_server(FakeBehavior::Reverse).await;

        let report = probe_fake_server(addr, Duration::from_millis(500)).await;

        assert_eq!(report.serial.verdict, ProbeVerdict::Reachable);
        assert_eq!(report.pipeline.verdict, ProbeVerdict::Supported);
        assert_eq!(report.pipeline.success_count, 4);
    }

    #[tokio::test]
    async fn probe_pipeline_unsupported_when_pipelined_responses_timeout() {
        let addr = start_fake_tcp_server(FakeBehavior::DropPipelined).await;

        let report = probe_fake_server(addr, Duration::from_millis(80)).await;

        assert_eq!(report.serial.verdict, ProbeVerdict::Reachable);
        assert_eq!(report.pipeline.verdict, ProbeVerdict::Unsupported);
        assert_eq!(report.pipeline.timeout_count, 4);
    }

    #[tokio::test]
    async fn probe_pipeline_unstable_when_questions_are_crossed() {
        let addr = start_fake_tcp_server(FakeBehavior::SwapQuestions).await;

        let report = probe_fake_server(addr, Duration::from_millis(500)).await;

        assert_eq!(report.serial.verdict, ProbeVerdict::Reachable);
        assert_eq!(report.pipeline.verdict, ProbeVerdict::Unstable);
        assert_eq!(report.pipeline.mismatch_count, 4);
    }

    #[tokio::test]
    async fn probe_pipeline_caps_unexpected_responses() {
        let addr = start_fake_tcp_server(FakeBehavior::UnexpectedFlood).await;

        let report = probe_fake_server(addr, Duration::from_millis(500)).await;

        assert_eq!(report.serial.verdict, ProbeVerdict::Reachable);
        assert_eq!(report.pipeline.verdict, ProbeVerdict::Unstable);
        assert_eq!(report.pipeline.mismatch_count, 4);
        assert_eq!(report.pipeline.error_count, 4);
        assert_eq!(report.pipeline.total_queries, 8);
        assert!(
            report
                .pipeline
                .errors
                .iter()
                .any(|error| error == "too many unexpected pipeline responses")
        );
    }

    #[tokio::test]
    async fn probe_udp_concurrency_supported() {
        let addr = start_fake_udp_server().await;

        let report = probe_upstream(UpstreamProbeConfig {
            upstream: make_upstream_config(format!("udp://{addr}"), Duration::from_millis(500)),
            qname: "example.com.".to_string(),
            qtype: RecordType::A,
            serial_samples: 1,
            pipeline_concurrency: 4,
            pipeline_rounds: 1,
        })
        .await
        .expect("probe should produce a report");

        assert_eq!(report.serial.verdict, ProbeVerdict::Reachable);
        assert_eq!(report.pipeline.verdict, ProbeVerdict::Supported);
        assert_eq!(report.pipeline.success_count, 4);
    }

    #[tokio::test]
    async fn probe_udp_concurrency_is_inconclusive_when_serial_fails() {
        let report = probe_upstream(UpstreamProbeConfig {
            upstream: make_upstream_config(
                "udp://127.0.0.1:9".to_string(),
                Duration::from_millis(20),
            ),
            qname: "example.com.".to_string(),
            qtype: RecordType::A,
            serial_samples: 1,
            pipeline_concurrency: 2,
            pipeline_rounds: 1,
        })
        .await
        .expect("probe should produce a report");

        assert_eq!(report.serial.verdict, ProbeVerdict::Unreachable);
        assert_eq!(report.pipeline.verdict, ProbeVerdict::Inconclusive);
    }

    #[tokio::test]
    async fn probe_concurrency_is_inconclusive_when_serial_is_partial() {
        let config = UpstreamProbeConfig {
            upstream: make_upstream_config(
                "tcp://127.0.0.1:9".to_string(),
                Duration::from_millis(10),
            ),
            qname: "example.com.".to_string(),
            qtype: RecordType::A,
            serial_samples: 2,
            pipeline_concurrency: 2,
            pipeline_rounds: 1,
        };
        let serial = ProbeStageReport::from_results(
            ProbeVerdict::Reachable,
            vec![
                ProbeQueryResult {
                    index: 0,
                    query_name: "example.com.".to_string(),
                    query_id: 1,
                    ok: true,
                    latency_ms: Some(1),
                    response_id: Some(1),
                    rcode: Some("NoError".to_string()),
                    answer_count: Some(1),
                    authoritative: Some(false),
                    truncated: Some(false),
                    recursion_available: Some(true),
                    error_kind: None,
                    error: None,
                },
                query_error_result(
                    1,
                    2,
                    "example.com.".to_string(),
                    ERROR_KIND_TIMEOUT,
                    "timed out waiting for serial response".to_string(),
                    Some(10),
                ),
            ],
        );
        let mut connection_info =
            ConnectionInfo::with_addr("tcp://127.0.0.1:9").expect("addr should parse");
        let mut progress = |_| {};

        let pipeline = run_pipeline_probe(
            &mut connection_info,
            &config,
            serial_baseline_is_clean(&serial),
            &mut progress,
        )
        .await;

        assert_eq!(serial.success_count, 1);
        assert!(!serial_baseline_is_clean(&serial));
        assert_eq!(pipeline.verdict, ProbeVerdict::Inconclusive);
        assert_eq!(pipeline.total_queries, 0);
        assert!(
            pipeline
                .errors
                .iter()
                .any(|error| error.contains("serial baseline was not fully successful"))
        );
    }

    #[test]
    fn parse_record_type_accepts_lowercase() {
        assert_eq!(parse_record_type("aaaa").unwrap(), RecordType::AAAA);
    }

    #[test]
    fn pipeline_name_uses_synthetic_prefix_when_it_fits() {
        let name = pipeline_name("example.com.", 1, 2).expect("pipeline name should parse");

        assert_eq!(name.to_fqdn(), "oxidns-probe-1-2.example.com.");
    }

    #[test]
    fn pipeline_name_falls_back_to_base_when_prefix_exceeds_dns_limit() {
        let base = format!(
            "{}.{}.{}.{}.",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        parse_name(&base).expect("base qname should be valid");

        let name = pipeline_name(&base, 0, 0).expect("fallback name should parse");

        assert_eq!(name.to_fqdn(), base);
    }

    #[test]
    fn pipeline_verdict_marks_mismatch_unstable() {
        assert_eq!(pipeline_verdict(4, 3, 0, 1), ProbeVerdict::Unstable);
    }

    #[test]
    fn system_resolution_failure_blocks_direct_probe_without_proxy() {
        let info =
            ConnectionInfo::with_addr("tcp://dns.example.invalid:53").expect("addr should parse");
        let resolution = ResolutionProbe {
            ip: None,
            source: Some("system".to_string()),
            error: Some("system resolver timed out after 10ms".to_string()),
            apply_to_connection: false,
        };

        assert!(resolution_blocks_direct_probe(&info, &resolution));
    }

    #[test]
    fn system_resolution_failure_does_not_block_proxied_probe() {
        let mut info =
            ConnectionInfo::with_addr("tcp://dns.example.invalid:53").expect("addr should parse");
        info.socks5 = Some(Socks5Opt {
            username: None,
            password: None,
            socket_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 1080)),
        });
        let resolution = ResolutionProbe {
            ip: None,
            source: Some("system".to_string()),
            error: Some("system resolver timed out after 10ms".to_string()),
            apply_to_connection: false,
        };

        assert!(!resolution_blocks_direct_probe(&info, &resolution));
    }

    #[tokio::test]
    async fn resolve_remote_ip_uses_proxy_source_without_system_lookup() {
        let mut info =
            ConnectionInfo::with_addr("tcp://dns.example.invalid:53").expect("addr should parse");
        info.socks5 = Some(Socks5Opt {
            username: None,
            password: None,
            socket_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 1080)),
        });
        info.timeout = Duration::from_nanos(1);

        let resolution = resolve_remote_ip(&info, false).await;

        assert_eq!(resolution.ip, None);
        assert_eq!(resolution.source.as_deref(), Some("proxy"));
        assert_eq!(resolution.error, None);
        assert!(!resolution.apply_to_connection);
    }

    #[tokio::test]
    async fn resolve_remote_ip_prefers_bootstrap_over_proxy_resolution() {
        let answer_ip = Ipv4Addr::new(192, 0, 2, 54);
        let resolver_addr = start_fake_bootstrap_resolver(answer_ip).await;
        let resolver = Arc::new(
            NameResolver::new(vec![resolver_addr.to_string()], Some(4))
                .expect("bootstrap resolver should build"),
        );
        let mut info =
            ConnectionInfo::with_addr("tcp://dns.example.invalid:53").expect("addr should parse");
        info.bootstrap = Some(resolver);
        info.socks5 = Some(Socks5Opt {
            username: None,
            password: None,
            socket_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 1080)),
        });
        info.timeout = Duration::from_millis(500);

        let resolution = resolve_remote_ip(&info, false).await;

        assert_eq!(resolution.ip, Some(IpAddr::V4(answer_ip)));
        assert_eq!(resolution.source.as_deref(), Some("bootstrap"));
        assert_eq!(resolution.error, None);
        assert!(resolution.apply_to_connection);
    }

    #[tokio::test]
    async fn resolve_probe_socks5_respects_timeout() {
        let started = Instant::now();

        let error = resolve_probe_socks5_with(
            "proxy.example.invalid:1080",
            Duration::from_millis(10),
            |_raw| {
                std::thread::sleep(Duration::from_millis(200));
                None
            },
        )
        .await
        .expect_err("slow SOCKS5 resolution should time out")
        .to_string();

        assert!(started.elapsed() < Duration::from_millis(150));
        assert!(error.contains("SOCKS5 proxy resolution timed out"));
    }

    #[tokio::test]
    async fn resolve_bootstrap_remote_ip_retries_bootstrap_resolution() {
        let answer_ip = Ipv4Addr::new(192, 0, 2, 53);
        let resolver_addr = start_fake_bootstrap_resolver(answer_ip).await;
        let resolver = Arc::new(
            NameResolver::new(vec![resolver_addr.to_string()], Some(4))
                .expect("bootstrap resolver should build"),
        );
        let mut info =
            ConnectionInfo::with_addr("tcp://dns.example.invalid:53").expect("addr should parse");
        info.bootstrap = Some(resolver);
        info.timeout = Duration::from_millis(500);

        resolve_bootstrap_remote_ip(&mut info)
            .await
            .expect("pipeline bootstrap retry should resolve");

        assert_eq!(info.remote_ip, Some(IpAddr::V4(answer_ip)));
        assert!(info.bootstrap.is_none());
        assert!(info.bootstrap_timeout.is_none());
    }

    #[test]
    fn refresh_target_from_connection_remote_ip_records_bootstrap_retry() {
        let mut target = UpstreamProbeTarget {
            address: "tcp://dns.example.invalid:53".to_string(),
            protocol: "tcp".to_string(),
            server_name: "dns.example.invalid".to_string(),
            port: 53,
            resolved_ip: None,
            resolution_source: Some("bootstrap".to_string()),
            uses_bootstrap: true,
            resolution_error: Some("transient bootstrap failure".to_string()),
        };
        let mut info =
            ConnectionInfo::with_addr("tcp://dns.example.invalid:53").expect("addr should parse");
        info.remote_ip = Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 53)));

        refresh_target_from_connection_remote_ip(&mut target, &info);

        assert_eq!(target.resolved_ip.as_deref(), Some("192.0.2.53"));
        assert_eq!(target.resolution_source.as_deref(), Some("bootstrap"));
        assert_eq!(target.resolution_error, None);
    }

    #[tokio::test]
    async fn refresh_target_bootstrap_resolution_retries_after_generic_success() {
        let answer_ip = Ipv4Addr::new(192, 0, 2, 53);
        let resolver_addr = start_fake_bootstrap_resolver(answer_ip).await;
        let resolver = Arc::new(
            NameResolver::new(vec![resolver_addr.to_string()], Some(4))
                .expect("bootstrap resolver should build"),
        );
        let mut target = UpstreamProbeTarget {
            address: "udp://dns.example.invalid:53".to_string(),
            protocol: "udp".to_string(),
            server_name: "dns.example.invalid".to_string(),
            port: 53,
            resolved_ip: None,
            resolution_source: Some("bootstrap".to_string()),
            uses_bootstrap: true,
            resolution_error: Some("transient bootstrap failure".to_string()),
        };
        let mut info =
            ConnectionInfo::with_addr("udp://dns.example.invalid:53").expect("addr should parse");
        info.bootstrap = Some(resolver);
        info.timeout = Duration::from_millis(500);
        let serial = ProbeStageReport::from_results(
            ProbeVerdict::Reachable,
            vec![ProbeQueryResult {
                index: 0,
                query_name: "example.com.".to_string(),
                query_id: 100,
                ok: true,
                latency_ms: Some(1),
                response_id: Some(100),
                rcode: Some("NOERROR".to_string()),
                answer_count: Some(1),
                authoritative: Some(false),
                truncated: Some(false),
                recursion_available: Some(true),
                error_kind: None,
                error: None,
            }],
        );
        let pipeline = PipelineProbeReport::inconclusive(1, 1, "not run".to_string());

        refresh_target_bootstrap_resolution(&mut target, &mut info, &serial, &pipeline).await;

        assert_eq!(target.resolved_ip.as_deref(), Some("192.0.2.53"));
        assert_eq!(target.resolution_source.as_deref(), Some("bootstrap"));
        assert_eq!(target.resolution_error, None);
        assert_eq!(info.remote_ip, Some(IpAddr::V4(answer_ip)));
        assert!(info.bootstrap.is_none());
        assert!(info.bootstrap_timeout.is_none());
    }

    #[test]
    fn validate_probe_config_rejects_excessive_pipeline_sample_product() {
        let config = UpstreamProbeConfig {
            upstream: make_upstream_config(
                "tcp://127.0.0.1:53".to_string(),
                Duration::from_millis(10),
            ),
            qname: "example.com.".to_string(),
            qtype: RecordType::A,
            serial_samples: 1,
            pipeline_concurrency: MAX_PROBE_SAMPLES,
            pipeline_rounds: 2,
        };

        let error = validate_probe_config(&config)
            .expect_err("excessive pipeline sample product should fail")
            .to_string();

        assert!(error.contains("pipeline_concurrency * pipeline_rounds"));
    }

    #[test]
    fn query_error_kind_recognizes_timed_out_errors() {
        assert_eq!(
            query_error_kind("UDP query timed out after retries"),
            ERROR_KIND_TIMEOUT
        );
    }

    #[test]
    fn connection_error_results_preserve_timeout_kind() {
        let config = UpstreamProbeConfig {
            upstream: make_upstream_config(
                "tcp://127.0.0.1:53".to_string(),
                Duration::from_millis(10),
            ),
            qname: "example.com.".to_string(),
            qtype: RecordType::A,
            serial_samples: 1,
            pipeline_concurrency: 2,
            pipeline_rounds: 1,
        };

        let results =
            connection_error_results(&config, 0, "DNS query timeout after 10ms".to_string());

        assert_eq!(count_error_kind(&results, ERROR_KIND_TIMEOUT), 2);
        assert!(
            results
                .iter()
                .all(|result| { result.error_kind.as_deref() == Some(ERROR_KIND_TIMEOUT) })
        );
    }

    #[test]
    fn pipeline_report_does_not_double_count_mismatches_as_other_errors() {
        let results = vec![query_error_result(
            0,
            1,
            "example.com.".to_string(),
            ERROR_KIND_MISMATCH,
            "response ID mismatch: expected 1, got 2".to_string(),
            Some(1),
        )];

        let report = PipelineProbeReport::from_results(true, 1, 1, results, Vec::new());

        assert_eq!(report.mismatch_count, 1);
        assert_eq!(report.error_count, 0);
        assert_eq!(report.verdict, ProbeVerdict::Unstable);
    }

    #[test]
    fn collect_errors_keeps_unique_messages() {
        let counter = Arc::new(AtomicUsize::new(0));
        let results = (0..3)
            .map(|index| {
                counter.fetch_add(1, Ordering::Relaxed);
                query_error_result(
                    index,
                    index as u16,
                    "example.com.".to_string(),
                    ERROR_KIND_QUERY,
                    "same".to_string(),
                    None,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(counter.load(Ordering::Relaxed), 3);
        assert_eq!(collect_errors(&results, Vec::new()), vec!["same"]);
    }
}
