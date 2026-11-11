// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `ecs_handler` executor plugin.
//!
//! Implements EDNS Client Subnet (ECS) processing for outgoing queries.
//!
//! Supported policies:
//! - `forward = true`: keep client-supplied ECS when present.
//! - `forward = false`: remove existing ECS before selecting a replacement.
//! - `preset`: use a fixed source IP when existing ECS is not preserved.
//! - `send = true`: otherwise synthesize ECS from the request-local client IP.
//! - non-IN queries never receive synthesized ECS; `forward = false` still
//!   removes existing ECS before downstream execution.
//!
//! Post-stage behavior:
//! - response ECS is preserved only when existing client ECS was retained for
//!   an IN query, avoiding disclosure of internally generated subnet metadata.
//! - request ECS remains available after execution; this plugin does not
//!   restore or clear the query when the downstream chain returns.

use std::net::IpAddr;

use async_trait::async_trait;
use serde::Deserialize;
use serde_yaml_ng::Value;

use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::ip::normalize_ipv4_mapped_ip;
use crate::plugin::executor::{ExecStep, Executor, ExecutorNext};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::proto::{ClientSubnet, DNSClass, Edns, EdnsCode, EdnsOption, Message};
use crate::{continue_next, plugin_factory};

#[derive(Debug, Clone, Deserialize, Default)]
struct EcsHandlerConfig {
    /// Keep client-supplied ECS option when present.
    #[serde(default)]
    forward: bool,
    /// Synthesize ECS when existing ECS is not preserved and no preset is set.
    #[serde(default)]
    send: bool,
    /// Optional fixed IP used as ECS source instead of client source IP.
    preset: Option<String>,
    /// Source prefix length for synthesized IPv4 ECS.
    mask4: Option<u8>,
    /// Source prefix length for synthesized IPv6 ECS.
    mask6: Option<u8>,
}

#[derive(Debug)]
struct EcsHandler {
    tag: String,
    forward: bool,
    send: bool,
    preset: Option<IpAddr>,
    mask4: u8,
    mask6: u8,
}

#[async_trait]
impl Plugin for EcsHandler {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn destroy(&self) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Executor for EcsHandler {
    fn with_next(&self) -> bool {
        true
    }

    #[hotpath::measure]
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        self.execute_with_next(context, None).await
    }

    #[hotpath::measure]
    async fn execute_with_next(
        &self,
        context: &mut DnsContext,
        next: Option<ExecutorNext>,
    ) -> Result<ExecStep> {
        let forwarded_client_ecs = self.prepare_request(context)?;
        let step = continue_next!(next, context)?;
        if forwarded_client_ecs {
            return Ok(step);
        }

        if let Some(response) = context.response_mut() {
            strip_ecs_from_message(response);
        }
        Ok(step)
    }
}

impl EcsHandler {
    fn prepare_request(&self, context: &mut DnsContext) -> Result<bool> {
        let has_ecs = request_has_ecs(&context.request);
        if has_ecs && !self.forward {
            strip_ecs_from_message(context.request_mut());
        }

        if context.request.first_qclass() != Some(DNSClass::IN) {
            return Ok(false);
        }

        if has_ecs && self.forward {
            return Ok(true);
        }

        let source_ip = if let Some(preset) = self.preset {
            Some(normalize_ipv4_mapped_ip(preset))
        } else if self.send {
            Some(normalize_ipv4_mapped_ip(context.peer_addr().ip()))
        } else {
            None
        };

        if let Some(source_ip) = source_ip {
            let mask = match source_ip {
                IpAddr::V4(_) => self.mask4,
                IpAddr::V6(_) => self.mask6,
            };
            let ecs = EdnsOption::Subnet(ClientSubnet::new(source_ip, mask, 0));
            let opt = ensure_opt_record(context.request_mut());
            opt.insert(ecs);
        }

        Ok(false)
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("ecs_handler")]
pub struct EcsHandlerFactory;

impl PluginFactory for EcsHandlerFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        let handler =
            parse_handler_from_value(plugin_config.tag.as_str(), plugin_config.args.clone())?;
        Ok(UninitializedPlugin::Executor(Box::new(handler)))
    }

    fn quick_setup(&self, tag: &str, param: Option<String>) -> Result<UninitializedPlugin> {
        // Quick setup syntax: `ecs [ip[/mask]]`.
        let preset = param
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                let (ip, _) = s.split_once('/').unwrap_or((s, ""));
                ip.parse::<IpAddr>().map_err(|error| {
                    DnsError::plugin(format!("invalid ecs_handler preset '{}': {}", ip, error))
                })
            })
            .transpose()?;

        Ok(UninitializedPlugin::Executor(Box::new(EcsHandler {
            tag: tag.to_string(),
            forward: false,
            send: false,
            preset,
            mask4: 24,
            mask6: 48,
        })))
    }
}

fn parse_handler_from_value(tag: &str, args: Option<Value>) -> Result<EcsHandler> {
    let cfg = match args {
        Some(args) => serde_yaml_ng::from_value::<EcsHandlerConfig>(args)
            .map_err(|e| DnsError::plugin(format!("failed to parse ecs_handler config: {}", e)))?,
        None => EcsHandlerConfig::default(),
    };

    let mask4 = cfg.mask4.unwrap_or(24);
    let mask6 = cfg.mask6.unwrap_or(48);

    if mask4 > 32 {
        return Err(DnsError::plugin(
            "ecs_handler mask4 must be in range 0..=32",
        ));
    }
    if mask6 > 128 {
        return Err(DnsError::plugin(
            "ecs_handler mask6 must be in range 0..=128",
        ));
    }

    let preset = cfg
        .preset
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(|v| {
            v.parse::<IpAddr>()
                .map_err(|e| DnsError::plugin(format!("invalid ecs_handler preset '{}': {}", v, e)))
        })
        .transpose()?;

    Ok(EcsHandler {
        tag: tag.to_string(),
        forward: cfg.forward,
        send: cfg.send,
        preset,
        mask4,
        mask6,
    })
}

fn request_has_ecs(message: &Message) -> bool {
    message
        .edns()
        .as_ref()
        .is_some_and(|edns| matches!(edns.option(EdnsCode::Subnet), Some(EdnsOption::Subnet(_))))
}

fn strip_ecs_from_message(message: &mut Message) {
    if let Some(edns) = message.edns_mut() {
        edns.remove(EdnsCode::Subnet);
    }
}

fn ensure_opt_record(message: &mut Message) -> &mut Edns {
    message.ensure_edns_mut()
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::*;
    use crate::core::context::DnsContext;
    use crate::proto::{Message, Name, Question, RecordType};

    #[test]
    fn test_parse_handler_from_value_validation() {
        assert!(parse_handler_from_value("ecs", None).is_ok());
        assert!(
            parse_handler_from_value("ecs", Some(serde_yaml_ng::from_str("mask4: 64").unwrap()),)
                .is_err()
        );
    }

    fn make_context(qclass: DNSClass) -> DnsContext {
        let mut request = Message::new();
        request.add_question(Question::new(
            Name::from_ascii("example.com.").unwrap(),
            RecordType::A,
            qclass,
        ));
        DnsContext::new(
            SocketAddr::from((Ipv4Addr::new(10, 1, 1, 9), 5353)),
            request,
        )
    }

    fn add_ecs_option(message: &mut Message, ip: IpAddr, mask: u8) {
        let opt = ensure_opt_record(message);
        opt.insert(EdnsOption::Subnet(ClientSubnet::new(ip, mask, 0)));
    }

    #[test]
    fn test_ecs_handler_source_priority() {
        let client_ecs = ClientSubnet::new("198.51.100.9".parse().unwrap(), 32, 0);
        for peer in ["192.0.2.42", "2001:db8::42", "::ffff:192.0.2.42"] {
            let peer: IpAddr = peer.parse().unwrap();
            for preset in [
                None,
                Some("203.0.113.10"),
                Some("2001:db8:1::10"),
                Some("::ffff:203.0.113.10"),
            ] {
                let preset = preset.map(|ip| ip.parse::<IpAddr>().unwrap());
                for forward in [false, true] {
                    for send in [false, true] {
                        for has_client_ecs in [false, true] {
                            let plugin = EcsHandler {
                                tag: "ecs".to_string(),
                                forward,
                                send,
                                preset,
                                mask4: 20,
                                mask6: 56,
                            };
                            let mut ctx = make_context(DNSClass::IN);
                            ctx.set_peer_addr(SocketAddr::new(peer, 5353));
                            if has_client_ecs {
                                ensure_opt_record(ctx.request_mut())
                                    .insert(EdnsOption::Subnet(client_ecs.clone()));
                            }
                            let forwarded = plugin.prepare_request(&mut ctx).unwrap();
                            assert_eq!(forwarded, forward && has_client_ecs);
                            let expected = if forwarded {
                                Some(client_ecs.clone())
                            } else {
                                preset.or(send.then_some(peer)).map(|ip| {
                                    let ip = normalize_ipv4_mapped_ip(ip);
                                    ClientSubnet::new(ip, if ip.is_ipv4() { 20 } else { 56 }, 0)
                                })
                            };
                            let actual = ctx
                                .request
                                .edns()
                                .as_ref()
                                .and_then(|edns| edns.option(EdnsCode::Subnet));
                            let expected = expected.map(EdnsOption::Subnet);
                            assert_eq!(
                                actual,
                                expected.as_ref(),
                                "peer={peer}, preset={preset:?}, forward={forward}, send={send}, client_ecs={has_client_ecs}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_ecs_handler_non_in_only_applies_strip_policy() {
        use crate::proto::EdnsLocal;

        for forward in [false, true] {
            for has_client_ecs in [false, true] {
                let plugin = EcsHandler {
                    tag: "ecs".to_string(),
                    forward,
                    send: true,
                    preset: Some("203.0.113.10".parse().unwrap()),
                    mask4: 24,
                    mask6: 48,
                };
                let mut ctx = make_context(DNSClass::CH);
                let opt = ensure_opt_record(ctx.request_mut());
                opt.set_dnssec_ok(true);
                opt.set_udp_payload_size(4096);
                let other = EdnsOption::Local(EdnsLocal::new(65001, vec![1, 2, 3]));
                opt.insert(other.clone());
                if has_client_ecs {
                    add_ecs_option(ctx.request_mut(), "198.51.100.9".parse().unwrap(), 24);
                }
                assert!(!plugin.prepare_request(&mut ctx).unwrap());
                assert_eq!(request_has_ecs(&ctx.request), forward && has_client_ecs);
                let opt = ctx.request.edns().as_ref().unwrap();
                assert!(opt.flags().dnssec_ok);
                assert_eq!(opt.udp_payload_size(), 4096);
                assert_eq!(opt.option(EdnsCode::Unknown(65001)), Some(&other));
            }
        }
    }

    #[test]
    fn test_ecs_handler_quick_setup_rejects_invalid_preset() {
        for param in [
            "203.0.113.999",
            "not-an-ip",
            "203.0.113.999/24",
            "/24",
            "2001:db8::gg",
        ] {
            let error = EcsHandlerFactory
                .quick_setup("ecs", Some(param.to_string()))
                .err()
                .expect("invalid preset must be rejected");
            assert!(
                error.to_string().contains("invalid ecs_handler preset"),
                "{error}"
            );
        }
    }

    #[tokio::test]
    async fn test_ecs_handler_quick_setup_presets_and_empty_args() {
        for (param, expected) in [
            (None, None),
            (Some(""), None),
            (Some("  "), None),
            (Some(" 203.0.113.10 "), Some(("203.0.113.10", 24))),
            (Some("203.0.113.10/16"), Some(("203.0.113.10", 24))),
            (Some("2001:db8::10/64"), Some(("2001:db8::10", 48))),
        ] {
            let UninitializedPlugin::Executor(plugin) = EcsHandlerFactory
                .quick_setup("ecs", param.map(str::to_string))
                .unwrap()
            else {
                panic!("ecs_handler must create an executor");
            };
            let mut ctx = make_context(DNSClass::IN);
            add_ecs_option(ctx.request_mut(), "198.51.100.9".parse().unwrap(), 24);
            plugin.execute(&mut ctx).await.unwrap();
            let actual = ctx
                .request
                .edns()
                .as_ref()
                .and_then(|edns| edns.option(EdnsCode::Subnet));
            let expected = expected.map(|(ip, mask)| {
                EdnsOption::Subnet(ClientSubnet::new(ip.parse().unwrap(), mask, 0))
            });
            assert_eq!(actual, expected.as_ref(), "param={param:?}");
        }
    }

    #[tokio::test]
    async fn test_ecs_handler_send_inserts_request_ecs_and_strips_response_ecs() {
        let plugin = EcsHandler {
            tag: "ecs".to_string(),
            forward: false,
            send: true,
            preset: None,
            mask4: 24,
            mask6: 48,
        };
        let mut ctx = make_context(DNSClass::IN);

        let mut response = Message::new();
        add_ecs_option(&mut response, IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 24);
        ctx.set_response(response);

        plugin
            .execute_with_next(&mut ctx, None)
            .await
            .expect("continuation execute should succeed");
        assert!(request_has_ecs(&ctx.request));
        assert!(
            !request_has_ecs(ctx.response().expect("response should exist")),
            "response ECS should be stripped when not forwarded from client"
        );
    }

    #[tokio::test]
    async fn test_ecs_handler_forward_keeps_client_and_response_ecs() {
        let plugin = EcsHandler {
            tag: "ecs".to_string(),
            forward: true,
            send: false,
            preset: None,
            mask4: 24,
            mask6: 48,
        };
        let mut ctx = make_context(DNSClass::IN);
        add_ecs_option(ctx.request_mut(), IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 24);

        let mut response = Message::new();
        add_ecs_option(&mut response, IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 24);
        ctx.set_response(response);

        plugin
            .execute_with_next(&mut ctx, None)
            .await
            .expect("continuation execute should succeed");
        assert!(request_has_ecs(&ctx.request));
        assert!(request_has_ecs(
            ctx.response().expect("response should exist")
        ));
    }

    #[tokio::test]
    async fn test_ecs_handler_strips_client_ecs_when_forward_disabled() {
        let plugin = EcsHandler {
            tag: "ecs".to_string(),
            forward: false,
            send: false,
            preset: None,
            mask4: 24,
            mask6: 48,
        };
        let mut ctx = make_context(DNSClass::IN);
        add_ecs_option(ctx.request_mut(), IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 24);

        plugin
            .execute_with_next(&mut ctx, None)
            .await
            .expect("continuation execute should succeed");
        assert!(!request_has_ecs(&ctx.request));
    }

    #[tokio::test]
    async fn test_ecs_handler_strips_client_ecs_from_request() {
        let plugin = EcsHandler {
            tag: "ecs".to_string(),
            forward: false,
            send: false,
            preset: None,
            mask4: 24,
            mask6: 48,
        };
        let mut ctx = make_context(DNSClass::IN);
        add_ecs_option(ctx.request_mut(), IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 24);

        plugin
            .execute_with_next(&mut ctx, None)
            .await
            .expect("continuation execute should succeed");
        assert!(!request_has_ecs(&ctx.request));
    }

    #[tokio::test]
    async fn test_ecs_handler_send_appends_ecs_to_existing_opt() {
        let plugin = EcsHandler {
            tag: "ecs".to_string(),
            forward: false,
            send: true,
            preset: None,
            mask4: 24,
            mask6: 48,
        };
        let mut ctx = make_context(DNSClass::IN);
        let _ = ensure_opt_record(ctx.request_mut());

        plugin
            .execute_with_next(&mut ctx, None)
            .await
            .expect("continuation execute should succeed");
        assert!(request_has_ecs(&ctx.request));
    }

    #[tokio::test]
    async fn test_ecs_handler_send_creates_opt_when_request_has_none() {
        let plugin = EcsHandler {
            tag: "ecs".to_string(),
            forward: false,
            send: true,
            preset: None,
            mask4: 24,
            mask6: 48,
        };
        let mut ctx = make_context(DNSClass::IN);

        plugin
            .execute_with_next(&mut ctx, None)
            .await
            .expect("continuation execute should succeed");
        assert!(request_has_ecs(&ctx.request));
        assert!(ctx.request.edns().is_some());
    }

    #[tokio::test]
    async fn test_ecs_handler_strips_ecs_from_response() {
        let plugin = EcsHandler {
            tag: "ecs".to_string(),
            forward: false,
            send: true,
            preset: None,
            mask4: 24,
            mask6: 48,
        };
        let mut ctx = make_context(DNSClass::IN);

        let mut response = Message::new();
        add_ecs_option(&mut response, IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 24);
        ctx.set_response(response);

        plugin
            .execute_with_next(&mut ctx, None)
            .await
            .expect("continuation execute should succeed");
        assert!(
            !request_has_ecs(ctx.response().expect("response should exist")),
            "response ECS should be stripped when not forwarded from client"
        );
    }
}
