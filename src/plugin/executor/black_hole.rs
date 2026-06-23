// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `black_hole` executor plugin.
//!
//! Intercepts matched DNS queries by returning a configured blocking response.
//!
//! Typical usage is ad-blocking / sinkhole policy where matched domains should
//! be answered locally without upstream queries.
//!
//! Behavior:
//! - `nxdomain` returns NXDOMAIN for every qtype.
//! - `nodata` returns NOERROR with no answers for every qtype.
//! - `refused` returns REFUSED for every qtype.
//! - `null` returns `0.0.0.0` / `::` for A/AAAA and NODATA otherwise.
//! - `custom` returns configured IPs for A/AAAA and NODATA otherwise.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use serde::Deserialize;
use serde_yaml_ng::Value;

use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::error::{DnsError, Result};
use crate::infra::observability::metrics::{
    MetricLabel, MetricSample, MetricSink, MetricSource, register_metric_source,
    unregister_metric_source,
};
use crate::plugin::executor::{ExecStep, Executor, synthetic_response};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::plugin_factory;
use crate::proto::{A, AAAA, Message, Question, RData, Rcode, RecordType};

const BLACK_HOLE_ANSWER_TTL: u32 = 300;

#[derive(Debug, Clone, Deserialize, Default)]
struct BlackHoleConfig {
    /// Blocking response mode.
    #[serde(default)]
    mode: Option<String>,
    /// IP addresses returned as synthesized black-hole answers.
    ///
    /// IPv4 values are used for A queries, IPv6 values for AAAA queries.
    /// When present without `mode`, legacy configuration maps to `custom`.
    #[serde(default)]
    ips: Vec<String>,
    /// Whether to stop the executor chain after producing a local answer.
    #[serde(default)]
    short_circuit: bool,
}

#[derive(Debug, Clone)]
struct BlackHoleSettings {
    mode: BlackHoleMode,
    ips: Vec<IpAddr>,
    short_circuit: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BlackHoleMode {
    NxDomain,
    NoData,
    Null,
    Custom,
    Refused,
}

#[derive(Debug)]
struct BlackHole {
    tag: String,
    mode: BlackHoleMode,
    ipv4: Vec<Arc<RData>>,
    ipv6: Vec<Arc<RData>>,
    short_circuit: bool,
    metrics: Arc<BlackHoleMetrics>,
}

#[derive(Debug)]
struct BlackHoleMetrics {
    tag: String,
    block_total: AtomicU64,
}

impl BlackHoleMetrics {
    fn new(tag: String) -> Self {
        Self {
            tag,
            block_total: AtomicU64::new(0),
        }
    }
}

impl MetricSource for BlackHoleMetrics {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn plugin_type(&self) -> &'static str {
        "black_hole"
    }

    fn collect(&self, sink: &mut dyn MetricSink) {
        let labels = [MetricLabel::new("plugin_tag", self.tag.as_str())];
        sink.emit(MetricSample::counter(
            "blackhole_block_total",
            "Total black_hole intercepted responses.",
            &labels,
            self.block_total.load(Ordering::Relaxed),
        ));
    }
}

#[async_trait]
impl Plugin for BlackHole {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        register_metric_source(self.metrics.clone())
    }

    async fn destroy(&self) -> Result<()> {
        unregister_metric_source(&self.tag);
        Ok(())
    }
}

#[async_trait]
impl Executor for BlackHole {
    #[hotpath::measure]
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        let Some(question) = context.request.first_question() else {
            return Ok(ExecStep::Next);
        };
        let response = self.build_response(&context.request, question)?;
        context.set_response(response);
        self.metrics.block_total.fetch_add(1, Ordering::Relaxed);

        if self.short_circuit {
            Ok(ExecStep::Stop)
        } else {
            Ok(ExecStep::Next)
        }
    }
}

impl BlackHole {
    fn build_response(&self, request: &Message, question: &Question) -> Result<Message> {
        match self.mode {
            BlackHoleMode::NxDomain => Ok(synthetic_response::default_nxdomain_response(
                request, question,
            )),
            BlackHoleMode::NoData => Ok(synthetic_response::default_nodata_response(
                request, question,
            )),
            BlackHoleMode::Refused => Ok(request.response(Rcode::Refused)),
            BlackHoleMode::Null | BlackHoleMode::Custom => {
                self.build_address_or_nodata_response(request, question)
            }
        }
    }

    fn build_address_or_nodata_response(
        &self,
        request: &Message,
        question: &Question,
    ) -> Result<Message> {
        match question.qtype() {
            RecordType::A if !self.ipv4.is_empty() => {
                Ok(request.address_response_rdata(question, BLACK_HOLE_ANSWER_TTL, &self.ipv4)?)
            }
            RecordType::AAAA if !self.ipv6.is_empty() => {
                Ok(request.address_response_rdata(question, BLACK_HOLE_ANSWER_TTL, &self.ipv6)?)
            }
            _ => Ok(synthetic_response::default_nodata_response(
                request, question,
            )),
        }
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("black_hole")]
pub struct BlackHoleFactory;

impl PluginFactory for BlackHoleFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        build_black_hole(
            plugin_config.tag.clone(),
            parse_config(plugin_config.args.clone())?,
        )
    }

    fn quick_setup(&self, tag: &str, param: Option<String>) -> Result<UninitializedPlugin> {
        build_black_hole(
            tag.to_string(),
            parse_quick_setup(param.as_deref().unwrap_or_default())?,
        )
    }
}

fn build_black_hole(tag: String, settings: BlackHoleSettings) -> Result<UninitializedPlugin> {
    let (ipv4, ipv6) = match settings.mode {
        BlackHoleMode::Null => split_ips(vec![
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        ]),
        _ => split_ips(settings.ips),
    };

    Ok(UninitializedPlugin::Executor(Box::new(BlackHole {
        tag: tag.clone(),
        mode: settings.mode,
        ipv4,
        ipv6,
        short_circuit: settings.short_circuit,
        metrics: Arc::new(BlackHoleMetrics::new(tag)),
    })))
}

fn parse_config(args: Option<Value>) -> Result<BlackHoleSettings> {
    let Some(args) = args else {
        return resolve_settings(None, Vec::new(), false);
    };

    if let Some(raw) = args.as_str() {
        return parse_quick_setup(raw);
    }

    if let Some(seq) = args.as_sequence() {
        let mut out = Vec::new();
        for item in seq {
            let token: &str = item
                .as_str()
                .ok_or_else(|| DnsError::plugin("black_hole args list must contain strings"))?;
            out.extend(parse_ip_tokens(
                split_tokens(token)
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            )?);
        }
        return resolve_settings(None, out, false);
    }

    let cfg: BlackHoleConfig = serde_yaml_ng::from_value(args)
        .map_err(|e| DnsError::plugin(format!("failed to parse black_hole config: {}", e)))?;
    let ips = parse_ip_tokens(cfg.ips)?;
    let mode = cfg.mode.as_deref().map(parse_black_hole_mode).transpose()?;
    resolve_settings(mode, ips, cfg.short_circuit)
}

fn parse_quick_setup(raw: &str) -> Result<BlackHoleSettings> {
    let (raw, short_circuit) = strip_short_circuit_suffix(raw)?;
    let mut tokens = split_tokens(&raw);
    let mode = if let Some(first) = tokens.first().copied() {
        if let Some(mode) = parse_black_hole_mode_token(first)? {
            tokens.remove(0);
            Some(mode)
        } else {
            None
        }
    } else {
        None
    };

    let ips = parse_ip_tokens(tokens.into_iter().map(str::to_string).collect())?;
    resolve_settings(mode, ips, short_circuit)
}

fn resolve_settings(
    mode: Option<BlackHoleMode>,
    ips: Vec<IpAddr>,
    short_circuit: bool,
) -> Result<BlackHoleSettings> {
    let mode = match mode {
        Some(mode) => {
            if mode != BlackHoleMode::Custom && !ips.is_empty() {
                return Err(DnsError::plugin(format!(
                    "black_hole mode '{}' does not accept ips; use mode: custom",
                    mode.as_str()
                )));
            }
            if mode == BlackHoleMode::Custom && ips.is_empty() {
                return Err(DnsError::plugin(
                    "black_hole mode 'custom' requires at least one ip",
                ));
            }
            mode
        }
        None if ips.is_empty() => BlackHoleMode::NxDomain,
        None => BlackHoleMode::Custom,
    };

    Ok(BlackHoleSettings {
        mode,
        ips,
        short_circuit,
    })
}

fn parse_black_hole_mode(raw: &str) -> Result<BlackHoleMode> {
    parse_black_hole_mode_token(raw)?.ok_or_else(|| {
        DnsError::plugin(format!(
            "invalid black_hole mode '{}', expected nxdomain, nodata, null, custom, or refused",
            raw
        ))
    })
}

fn parse_black_hole_mode_token(raw: &str) -> Result<Option<BlackHoleMode>> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "nxdomain" => Ok(Some(BlackHoleMode::NxDomain)),
        "nodata" => Ok(Some(BlackHoleMode::NoData)),
        "null" => Ok(Some(BlackHoleMode::Null)),
        "custom" => Ok(Some(BlackHoleMode::Custom)),
        "refused" => Ok(Some(BlackHoleMode::Refused)),
        "" => Ok(None),
        _ => Ok(None),
    }
}

impl BlackHoleMode {
    fn as_str(self) -> &'static str {
        match self {
            BlackHoleMode::NxDomain => "nxdomain",
            BlackHoleMode::NoData => "nodata",
            BlackHoleMode::Null => "null",
            BlackHoleMode::Custom => "custom",
            BlackHoleMode::Refused => "refused",
        }
    }
}

fn parse_ip_tokens(raw_tokens: Vec<String>) -> Result<Vec<IpAddr>> {
    let mut out = Vec::with_capacity(raw_tokens.len());
    for token in raw_tokens {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let ip = token
            .parse::<IpAddr>()
            .map_err(|e| DnsError::plugin(format!("invalid black_hole ip '{}': {}", token, e)))?;
        out.push(ip);
    }
    Ok(out)
}

fn split_tokens(raw: &str) -> Vec<&str> {
    raw.split(|c: char| c == ',' || c.is_ascii_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

fn strip_short_circuit_suffix(raw: &str) -> Result<(String, bool)> {
    let mut tokens: Vec<&str> = raw.split_whitespace().collect();
    let mut short_circuit = false;

    while let Some(last) = tokens.last().copied() {
        let Some(value) = parse_short_circuit_token(last)? else {
            break;
        };
        short_circuit = value;
        tokens.pop();
    }

    Ok((tokens.join(" "), short_circuit))
}

fn parse_short_circuit_token(token: &str) -> Result<Option<bool>> {
    if token == "short_circuit" {
        return Ok(Some(true));
    }

    let Some(value) = token.strip_prefix("short_circuit=") else {
        return Ok(None);
    };

    match value {
        "true" => Ok(Some(true)),
        "false" => Ok(Some(false)),
        _ => Err(DnsError::plugin(format!(
            "invalid short_circuit value '{}', expected true or false",
            value
        ))),
    }
}

fn split_ips(ips: Vec<IpAddr>) -> (Vec<Arc<RData>>, Vec<Arc<RData>>) {
    let mut ipv4 = Vec::new();
    let mut ipv6 = Vec::new();

    for ip in ips {
        match ip {
            IpAddr::V4(v4) => ipv4.push(Arc::new(RData::A(A(v4)))),
            IpAddr::V6(v6) => ipv6.push(Arc::new(RData::AAAA(AAAA(v6)))),
        }
    }

    (ipv4, ipv6)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    use super::*;
    use crate::plugin::UninitializedPlugin;
    use crate::plugin::executor::ExecStep;
    use crate::proto::{DNSClass, Message, Name, Question};

    fn make_context(qtype: RecordType) -> DnsContext {
        let mut request = Message::new();
        request.add_question(Question::new(
            Name::from_ascii("example.com.").unwrap(),
            qtype,
            DNSClass::IN,
        ));

        DnsContext::new(SocketAddr::from((Ipv4Addr::LOCALHOST, 5300)), request)
    }

    fn make_empty_context() -> DnsContext {
        DnsContext::new(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 5300)),
            Message::new(),
        )
    }

    fn test_metrics() -> Arc<BlackHoleMetrics> {
        Arc::new(BlackHoleMetrics::new("bh".to_string()))
    }

    fn make_plugin(
        mode: BlackHoleMode,
        ips: Vec<IpAddr>,
        short_circuit: bool,
        metrics: Arc<BlackHoleMetrics>,
    ) -> BlackHole {
        let (ipv4, ipv6) = match mode {
            BlackHoleMode::Null => split_ips(vec![
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            ]),
            _ => split_ips(ips),
        };

        BlackHole {
            tag: "bh".to_string(),
            mode,
            ipv4,
            ipv6,
            short_circuit,
            metrics,
        }
    }

    fn assert_fake_soa(response: &Message) {
        assert_eq!(response.authorities().len(), 1);
        assert_eq!(response.authorities()[0].rr_type(), RecordType::SOA);
        assert_eq!(
            response.authorities()[0].ttl(),
            synthetic_response::DEFAULT_FAKE_SOA_TTL
        );
    }

    #[test]
    fn test_parse_ip_tokens_validation() {
        assert!(parse_ip_tokens(vec![]).is_ok());
        assert!(parse_ip_tokens(vec!["invalid".to_string()]).is_err());
        assert!(parse_ip_tokens(vec!["1.1.1.1".to_string()]).is_ok());
    }

    #[test]
    fn test_parse_config_defaults_to_nxdomain() {
        let cfg = parse_config(None).expect("default config should parse");
        assert_eq!(cfg.mode, BlackHoleMode::NxDomain);
        assert!(cfg.ips.is_empty());
        assert!(!cfg.short_circuit);
    }

    #[test]
    fn test_parse_config_maps_legacy_ips_to_custom() {
        let value: Value = serde_yaml_ng::from_str(
            r#"
ips:
  - "0.0.0.0"
  - "::"
short_circuit: true
"#,
        )
        .expect("yaml should parse");

        let cfg = parse_config(Some(value)).expect("legacy config should parse");

        assert_eq!(cfg.mode, BlackHoleMode::Custom);
        assert_eq!(cfg.ips.len(), 2);
        assert!(cfg.short_circuit);
    }

    #[test]
    fn test_parse_config_accepts_case_insensitive_mode() {
        let value: Value = serde_yaml_ng::from_str(
            r#"
mode: NoData
"#,
        )
        .expect("yaml should parse");

        let cfg = parse_config(Some(value)).expect("mode should parse");

        assert_eq!(cfg.mode, BlackHoleMode::NoData);
    }

    #[test]
    fn test_parse_config_rejects_ips_for_non_custom_mode() {
        let value: Value = serde_yaml_ng::from_str(
            r#"
mode: refused
ips:
  - "0.0.0.0"
"#,
        )
        .expect("yaml should parse");

        assert!(parse_config(Some(value)).is_err());
    }

    #[test]
    fn test_parse_config_rejects_empty_custom_mode() {
        let value: Value = serde_yaml_ng::from_str(
            r#"
mode: custom
"#,
        )
        .expect("yaml should parse");

        assert!(parse_config(Some(value)).is_err());
    }

    #[test]
    fn test_parse_quick_setup_modes_and_legacy_ips() {
        let default_cfg = parse_quick_setup("").expect("empty quick setup should parse");
        assert_eq!(default_cfg.mode, BlackHoleMode::NxDomain);

        let nxdomain =
            parse_quick_setup("nxdomain short_circuit=true").expect("nxdomain should parse");
        assert_eq!(nxdomain.mode, BlackHoleMode::NxDomain);
        assert!(nxdomain.short_circuit);

        let explicit_custom =
            parse_quick_setup("custom 0.0.0.0 :: short_circuit=true").expect("custom should parse");
        assert_eq!(explicit_custom.mode, BlackHoleMode::Custom);
        assert_eq!(explicit_custom.ips.len(), 2);
        assert!(explicit_custom.short_circuit);

        let legacy =
            parse_quick_setup("0.0.0.0 :: short_circuit=true").expect("legacy should parse");
        assert_eq!(legacy.mode, BlackHoleMode::Custom);
        assert_eq!(legacy.ips.len(), 2);
        assert!(legacy.short_circuit);
    }

    #[tokio::test]
    async fn test_black_hole_quick_setup_supports_short_circuit() {
        let plugin = BlackHoleFactory
            .quick_setup("bh_quick", Some("0.0.0.0 short_circuit=true".to_string()))
            .expect("quick setup should succeed");

        let UninitializedPlugin::Executor(plugin) = plugin else {
            panic!("expected executor plugin");
        };
        let mut ctx = make_context(RecordType::A);
        let step = plugin.execute(&mut ctx).await.expect("execute should work");
        assert!(matches!(step, ExecStep::Stop));
        assert!(ctx.response().is_some());
    }

    #[tokio::test]
    async fn test_black_hole_execute_generates_custom_a_answers() {
        let metrics = test_metrics();
        let plugin = make_plugin(
            BlackHoleMode::Custom,
            vec![IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))],
            false,
            metrics.clone(),
        );
        let mut ctx = make_context(RecordType::A);
        let step = plugin
            .execute(&mut ctx)
            .await
            .expect("execute should succeed");
        assert!(matches!(step, ExecStep::Next));
        let resp = ctx.response().expect("response should exist");
        assert_eq!(resp.answers().len(), 1);
        assert_eq!(resp.answers()[0].rr_type(), RecordType::A);
        assert_eq!(
            resp.answers()[0].ip_addr(),
            Some(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)))
        );
        assert_eq!(metrics.block_total.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_black_hole_metrics_ignore_empty_request_pass_through() {
        let metrics = test_metrics();
        let plugin = make_plugin(BlackHoleMode::NxDomain, vec![], false, metrics.clone());

        let mut ctx = make_empty_context();
        let step = plugin
            .execute(&mut ctx)
            .await
            .expect("execute should succeed");

        assert!(matches!(step, ExecStep::Next));
        assert!(ctx.response().is_none());
        assert_eq!(metrics.block_total.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_black_hole_execute_generates_custom_aaaa_answers() {
        let plugin = make_plugin(
            BlackHoleMode::Custom,
            vec![IpAddr::V6(Ipv6Addr::LOCALHOST)],
            false,
            test_metrics(),
        );
        let mut ctx = make_context(RecordType::AAAA);
        let step = plugin
            .execute(&mut ctx)
            .await
            .expect("execute should succeed");
        assert!(matches!(step, ExecStep::Next));
        let resp = ctx.response().expect("response should exist");
        assert_eq!(resp.answers().len(), 1);
        assert_eq!(resp.answers()[0].rr_type(), RecordType::AAAA);
        assert_eq!(
            resp.answers()[0].ip_addr(),
            Some(IpAddr::V6(Ipv6Addr::LOCALHOST))
        );
    }

    #[tokio::test]
    async fn test_black_hole_custom_missing_family_returns_nodata() {
        let plugin = make_plugin(
            BlackHoleMode::Custom,
            vec![IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))],
            false,
            test_metrics(),
        );
        let mut ctx = make_context(RecordType::AAAA);

        let step = plugin
            .execute(&mut ctx)
            .await
            .expect("execute should succeed");
        assert!(matches!(step, ExecStep::Next));
        let resp = ctx.response().expect("response should exist");
        assert_eq!(resp.rcode(), Rcode::NoError);
        assert!(resp.answers().is_empty());
        assert_fake_soa(resp);
    }

    #[tokio::test]
    async fn test_black_hole_custom_non_address_qtype_returns_nodata() {
        let plugin = make_plugin(
            BlackHoleMode::Custom,
            vec![IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))],
            false,
            test_metrics(),
        );
        let mut ctx = make_context(RecordType::TXT);

        let step = plugin
            .execute(&mut ctx)
            .await
            .expect("execute should succeed");
        assert!(matches!(step, ExecStep::Next));
        let resp = ctx.response().expect("response should exist");
        assert_eq!(resp.rcode(), Rcode::NoError);
        assert!(resp.answers().is_empty());
        assert_fake_soa(resp);
    }

    #[tokio::test]
    async fn test_black_hole_null_mode_returns_unspecified_addresses() {
        let plugin = make_plugin(BlackHoleMode::Null, vec![], false, test_metrics());

        let mut a_ctx = make_context(RecordType::A);
        plugin
            .execute(&mut a_ctx)
            .await
            .expect("A execute should succeed");
        let a_resp = a_ctx.response().expect("A response should exist");
        assert_eq!(a_resp.answers().len(), 1);
        assert_eq!(
            a_resp.answers()[0].ip_addr(),
            Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        );

        let mut aaaa_ctx = make_context(RecordType::AAAA);
        plugin
            .execute(&mut aaaa_ctx)
            .await
            .expect("AAAA execute should succeed");
        let aaaa_resp = aaaa_ctx.response().expect("AAAA response should exist");
        assert_eq!(aaaa_resp.answers().len(), 1);
        assert_eq!(
            aaaa_resp.answers()[0].ip_addr(),
            Some(IpAddr::V6(Ipv6Addr::UNSPECIFIED))
        );

        let mut txt_ctx = make_context(RecordType::TXT);
        plugin
            .execute(&mut txt_ctx)
            .await
            .expect("TXT execute should succeed");
        let txt_resp = txt_ctx.response().expect("TXT response should exist");
        assert_eq!(txt_resp.rcode(), Rcode::NoError);
        assert!(txt_resp.answers().is_empty());
        assert_fake_soa(txt_resp);
    }

    #[tokio::test]
    async fn test_black_hole_nxdomain_mode_covers_non_address_qtype() {
        let metrics = test_metrics();
        let plugin = make_plugin(BlackHoleMode::NxDomain, vec![], false, metrics.clone());
        let mut ctx = make_context(RecordType::TXT);

        let step = plugin
            .execute(&mut ctx)
            .await
            .expect("execute should succeed");

        assert!(matches!(step, ExecStep::Next));
        let resp = ctx.response().expect("response should exist");
        assert_eq!(resp.rcode(), Rcode::NXDomain);
        assert!(resp.answers().is_empty());
        assert_fake_soa(resp);
        assert_eq!(metrics.block_total.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_black_hole_nodata_mode_covers_non_address_qtype() {
        let plugin = make_plugin(BlackHoleMode::NoData, vec![], false, test_metrics());
        let mut ctx = make_context(RecordType::TXT);

        plugin
            .execute(&mut ctx)
            .await
            .expect("execute should succeed");

        let resp = ctx.response().expect("response should exist");
        assert_eq!(resp.rcode(), Rcode::NoError);
        assert!(resp.answers().is_empty());
        assert_fake_soa(resp);
    }

    #[tokio::test]
    async fn test_black_hole_refused_mode_returns_refused_without_soa() {
        let plugin = make_plugin(BlackHoleMode::Refused, vec![], false, test_metrics());
        let mut ctx = make_context(RecordType::TXT);

        plugin
            .execute(&mut ctx)
            .await
            .expect("execute should succeed");

        let resp = ctx.response().expect("response should exist");
        assert_eq!(resp.rcode(), Rcode::Refused);
        assert!(resp.answers().is_empty());
        assert!(resp.authorities().is_empty());
    }

    #[tokio::test]
    async fn test_black_hole_execute_uses_first_question_for_multi_question_request() {
        let plugin = make_plugin(
            BlackHoleMode::Custom,
            vec![IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))],
            false,
            test_metrics(),
        );
        let mut ctx = make_context(RecordType::A);
        ctx.request.questions_mut().push(Question::new(
            Name::from_ascii("example.org.").unwrap(),
            RecordType::AAAA,
            DNSClass::IN,
        ));

        let step = plugin
            .execute(&mut ctx)
            .await
            .expect("execute should succeed");

        assert!(matches!(step, ExecStep::Next));
        let resp = ctx.response().expect("response should exist");
        assert_eq!(resp.answers().len(), 1);
        assert_eq!(resp.answers()[0].rr_type(), RecordType::A);
    }

    #[tokio::test]
    async fn test_black_hole_execute_stops_when_short_circuit_enabled() {
        let plugin = make_plugin(BlackHoleMode::NoData, vec![], true, test_metrics());
        let mut ctx = make_context(RecordType::TXT);
        let step = plugin
            .execute(&mut ctx)
            .await
            .expect("execute should succeed");
        assert!(matches!(step, ExecStep::Stop));
        assert!(ctx.response().is_some());
    }
}
