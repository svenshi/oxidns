// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! RouterOS route keys and ownership metadata.

use std::net::IpAddr;

use crate::infra::error::{DnsError, Result};
use crate::plugin::executor::routeros::ip_prefix::{IpPrefix, host_prefix};
use crate::plugin::executor::routeros::lease::LeaseDeadline;

const COMMENT_FIELD_PLUGIN: &str = "pg";
const COMMENT_FIELD_KIND: &str = "kind";
const COMMENT_FIELD_EXP: &str = "exp";
const COMMENT_FIELD_SEEN: &str = "seen";
const COMMENT_KIND_DYNAMIC: &str = "D";
const COMMENT_KIND_PERSISTENT: &str = "P";
const COMMENT_KIND_GATEWAY_CHECK: &str = "V";

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub(super) enum RouteFamily {
    Ipv4,
    Ipv6,
}

impl RouteFamily {
    pub(super) fn from_ip(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(_) => Self::Ipv4,
            IpAddr::V6(_) => Self::Ipv6,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub(super) struct RouteKey {
    pub(super) ip: IpAddr,
    pub(super) prefix: u8,
    pub(super) table: String,
}

impl RouteKey {
    pub(super) fn new(ip: IpAddr, table: String) -> Self {
        Self {
            ip,
            prefix: host_prefix(ip),
            table,
        }
    }

    pub(super) fn family(&self) -> RouteFamily {
        RouteFamily::from_ip(self.ip)
    }

    pub(super) fn dst_address(&self) -> String {
        format!("{}/{}", self.ip, self.prefix)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum RouteCommentKind {
    Dynamic,
    Persistent,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct RouteCommentMeta {
    pub(super) kind: RouteCommentKind,
    pub(super) expires_at_ms: LeaseDeadline,
    pub(super) last_seen_ms: u64,
}

#[derive(Debug)]
pub(super) struct RouteCommentCodec;

impl RouteCommentCodec {
    pub(super) fn prefix(prefix: &str, plugin_tag: &str, kind: &str) -> String {
        let mut out = String::new();
        if !prefix.is_empty() {
            out.push_str(prefix);
            out.push(';');
        }
        out.push_str(COMMENT_FIELD_PLUGIN);
        out.push('=');
        out.push_str(plugin_tag);
        out.push(';');
        out.push_str(COMMENT_FIELD_KIND);
        out.push('=');
        out.push_str(kind);
        out
    }

    pub(super) fn encode_persistent(prefix: &str, plugin_tag: &str) -> String {
        Self::prefix(prefix, plugin_tag, COMMENT_KIND_PERSISTENT)
    }

    pub(super) fn encode_dynamic(
        prefix: &str,
        plugin_tag: &str,
        deadline: LeaseDeadline,
        last_seen_ms: u64,
    ) -> String {
        let mut out = Self::prefix(prefix, plugin_tag, COMMENT_KIND_DYNAMIC);
        let expires_at = deadline.unix_millis().map_or(0, |value| value / 1_000);
        out.push(';');
        out.push_str(COMMENT_FIELD_EXP);
        out.push('=');
        out.push_str(&expires_at.to_string());
        out.push(';');
        out.push_str(COMMENT_FIELD_SEEN);
        out.push('=');
        out.push_str(&(last_seen_ms / 1_000).to_string());
        out
    }

    pub(super) fn decode(
        prefix: &str,
        plugin_tag: &str,
        family: RouteFamily,
        dst_address: &str,
        comment: &str,
    ) -> Result<Option<RouteCommentMeta>> {
        if !prefix.is_empty()
            && (!comment.starts_with(prefix) || comment.as_bytes().get(prefix.len()) != Some(&b';'))
        {
            return Ok(None);
        }
        let mut owner = None;
        let mut kind = None;
        let mut expiry = None;
        let mut seen = None;
        for token in comment.split(';') {
            let Some((key, value)) = token.trim().split_once('=') else {
                continue;
            };
            match key.trim() {
                COMMENT_FIELD_PLUGIN => owner = Some(value.trim()),
                COMMENT_FIELD_KIND => kind = Some(value.trim()),
                COMMENT_FIELD_EXP => expiry = Some(value.trim()),
                COMMENT_FIELD_SEEN => seen = Some(value.trim()),
                _ => {}
            }
        }
        if owner != Some(plugin_tag) {
            return Ok(None);
        }
        let prefix = dst_address
            .parse::<IpPrefix>()
            .map_err(|error| DnsError::plugin(format!("ros_route invalid dst-address: {error}")))?;
        if RouteFamily::from_ip(prefix.address()) != family {
            return Err(DnsError::plugin(
                "ros_route comment address family mismatch",
            ));
        }
        match kind {
            Some(COMMENT_KIND_PERSISTENT) => Ok(Some(RouteCommentMeta {
                kind: RouteCommentKind::Persistent,
                expires_at_ms: LeaseDeadline::Timeless,
                last_seen_ms: 0,
            })),
            Some(COMMENT_KIND_DYNAMIC) if prefix.is_host() => {
                let expiry = expiry
                    .ok_or_else(|| DnsError::plugin("ros_route dynamic comment missing exp"))?
                    .parse::<u64>()
                    .map_err(|error| DnsError::plugin(format!("ros_route invalid exp: {error}")))?;
                let seen = seen
                    .ok_or_else(|| DnsError::plugin("ros_route dynamic comment missing seen"))?
                    .parse::<u64>()
                    .map_err(|error| {
                        DnsError::plugin(format!("ros_route invalid seen: {error}"))
                    })?;
                Ok(Some(RouteCommentMeta {
                    kind: RouteCommentKind::Dynamic,
                    expires_at_ms: if expiry == 0 {
                        LeaseDeadline::Timeless
                    } else {
                        LeaseDeadline::At(expiry.saturating_mul(1_000))
                    },
                    last_seen_ms: seen.saturating_mul(1_000),
                }))
            }
            Some(COMMENT_KIND_DYNAMIC) => Err(DnsError::plugin(
                "ros_route dynamic comment is only valid for host routes",
            )),
            _ => Ok(None),
        }
    }
}

pub(super) fn validation_comment(prefix: &str, plugin_tag: &str) -> String {
    RouteCommentCodec::prefix(prefix, plugin_tag, COMMENT_KIND_GATEWAY_CHECK)
}

pub(super) fn is_validation_comment(prefix: &str, plugin_tag: &str, comment: &str) -> bool {
    let marker = validation_comment(prefix, plugin_tag);
    comment == marker || comment.starts_with(&format!("{marker};nonce="))
}
