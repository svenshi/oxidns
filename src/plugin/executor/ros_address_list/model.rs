//! RouterOS address-list keys and ownership metadata.

use std::net::IpAddr;

use crate::plugin::executor::routeros::ip_prefix::IpPrefix;

const HOST_PREFIX_V4: u8 = 32;
const HOST_PREFIX_V6: u8 = 128;
const COMMENT_FIELD_PLUGIN: &str = "pg";
const COMMENT_FIELD_KIND: &str = "kind";
const COMMENT_KIND_DYNAMIC: &str = "D";
const COMMENT_KIND_PERSISTENT: &str = "P";

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub(super) enum AddressListFamily {
    Ipv4,
    Ipv6,
}

impl AddressListFamily {
    pub(super) fn from_ip(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(_) => Self::Ipv4,
            IpAddr::V6(_) => Self::Ipv6,
        }
    }

    pub(super) fn host_prefix(self) -> u8 {
        match self {
            Self::Ipv4 => HOST_PREFIX_V4,
            Self::Ipv6 => HOST_PREFIX_V6,
        }
    }

    pub(super) fn is_valid_prefix(self, prefix: u8) -> bool {
        match self {
            Self::Ipv4 => prefix <= HOST_PREFIX_V4,
            Self::Ipv6 => prefix <= HOST_PREFIX_V6,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub(super) struct AddressListKey {
    pub(super) family: AddressListFamily,
    pub(super) list: String,
    pub(super) address: IpAddr,
    pub(super) prefix: u8,
}

impl AddressListKey {
    pub(super) fn new(ip: IpAddr, list: String) -> Self {
        let family = AddressListFamily::from_ip(ip);
        let prefix = IpPrefix::host(ip);
        Self {
            family,
            list,
            address: prefix.address(),
            prefix: prefix.prefix(),
        }
    }

    pub(super) fn new_with_prefix(ip: IpAddr, prefix: u8, list: String) -> Option<Self> {
        let normalized = IpPrefix::new(ip, prefix)?;
        let family = AddressListFamily::from_ip(normalized.address());
        Some(Self {
            family,
            list,
            address: normalized.address(),
            prefix: normalized.prefix(),
        })
    }

    pub(super) fn normalized_value(&self) -> String {
        format!("{}/{}", self.address, self.prefix)
    }

    /// Address text in the form RouterOS stores and matches for this family.
    ///
    /// RouterOS keeps IPv4 host entries without a mask and rewrites an
    /// explicit `/32` to the bare address, while IPv6 host entries are kept in
    /// `/128` form and an explicit bare address is rewritten to `/128`.
    /// Lookups use string equality against the stored property, so addressing
    /// RouterOS in any other form makes a row invisible to its own writer.
    pub(super) fn router_value(&self) -> String {
        match self.family {
            AddressListFamily::Ipv4 if self.prefix == HOST_PREFIX_V4 => self.address.to_string(),
            _ => self.normalized_value(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum OwnedCommentKind {
    Dynamic,
    Persistent,
}

impl OwnedCommentKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Dynamic => COMMENT_KIND_DYNAMIC,
            Self::Persistent => COMMENT_KIND_PERSISTENT,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct OwnedCommentMeta {
    pub(super) kind: OwnedCommentKind,
}

pub(super) fn encode_comment(prefix: &str, plugin_tag: &str, kind: OwnedCommentKind) -> String {
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
    out.push_str(kind.as_str());
    out
}

pub(super) fn decode_owned_comment(
    prefix: &str,
    plugin_tag: &str,
    comment: Option<&str>,
) -> Option<OwnedCommentMeta> {
    let comment = comment?;
    if !prefix.is_empty()
        && (!comment.starts_with(prefix) || comment.as_bytes().get(prefix.len()) != Some(&b';'))
    {
        return None;
    }

    let mut plugin_matches = false;
    let mut kind = None;
    for token in comment.split(';') {
        let Some((key, value)) = token.trim().split_once('=') else {
            continue;
        };
        match key.trim() {
            COMMENT_FIELD_PLUGIN if value.trim() == plugin_tag => plugin_matches = true,
            COMMENT_FIELD_KIND => {
                kind = match value.trim() {
                    COMMENT_KIND_DYNAMIC => Some(OwnedCommentKind::Dynamic),
                    COMMENT_KIND_PERSISTENT => Some(OwnedCommentKind::Persistent),
                    _ => None,
                };
            }
            _ => {}
        }
    }
    plugin_matches
        .then(|| kind.map(|kind| OwnedCommentMeta { kind }))
        .flatten()
}

pub(super) fn parse_router_address(family: AddressListFamily, raw: &str) -> Option<(IpAddr, u8)> {
    let value = raw.trim();
    if let Some((ip_raw, prefix_raw)) = value.split_once('/') {
        let ip = ip_raw.parse::<IpAddr>().ok()?;
        let prefix = prefix_raw.parse::<u8>().ok()?;
        if AddressListFamily::from_ip(ip) != family || !family.is_valid_prefix(prefix) {
            return None;
        }
        let normalized = IpPrefix::new(ip, prefix)?;
        return Some((normalized.address(), normalized.prefix()));
    }
    let ip = value.parse::<IpAddr>().ok()?;
    (AddressListFamily::from_ip(ip) == family).then(|| (ip, family.host_prefix()))
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn router_values_use_the_form_routeros_stores_per_family() {
        let v4_host = AddressListKey::new(
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
            "policy".to_string(),
        );
        assert_eq!(v4_host.router_value(), "192.0.2.10");

        let v6_host = AddressListKey::new(
            IpAddr::V6("2001:db8::1".parse::<Ipv6Addr>().expect("ipv6")),
            "policy6".to_string(),
        );
        assert_eq!(v6_host.router_value(), "2001:db8::1/128");

        let v4_network = AddressListKey::new_with_prefix(
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 129)),
            24,
            "policy".to_string(),
        )
        .expect("network");
        assert_eq!(v4_network.router_value(), "192.0.2.0/24");
    }
}
