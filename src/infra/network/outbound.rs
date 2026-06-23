// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared outbound connection profiles.
//!
//! Outbound profiles describe how process-owned clients connect to external
//! services: which resolver to use and whether a proxy is involved. Callers
//! such as the shared HTTP client consume the resolved runtime policy instead
//! of parsing SOCKS5 or nameserver settings on their own.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::config::types::{
    NetworkOutboundConfig, OutboundNameserverConfig, OutboundProfileConfig, OutboundProxyConfig,
    OutboundResolverConfig, OutboundResolverDetailedConfig, OutboundResolverProxyConfig,
};
use crate::infra::error::{DnsError, Result};
use crate::infra::network::deadline::QueryDeadline;
use crate::infra::network::metrics::{self as network_metrics, NetworkProfileMetrics};
use crate::infra::network::proxy::{Socks5Opt, parse_socks5_opt};
use crate::infra::network::resolver::{NameResolver, NameserverConfig};
use crate::infra::system::parse_simple_duration;

const DEFAULT_BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub(crate) struct OutboundPolicy {
    resolver: ResolverPolicy,
    proxy: ProxyPolicy,
}

impl OutboundPolicy {
    pub(crate) fn system(proxy: Option<Socks5Opt>) -> Self {
        Self {
            resolver: ResolverPolicy::System,
            proxy: ProxyPolicy::from_socks5(proxy),
        }
    }

    pub(crate) fn proxy(&self) -> Option<Socks5Opt> {
        self.proxy.socks5()
    }

    pub(crate) fn resolver(&self) -> Option<(Arc<NameResolver>, Duration)> {
        match &self.resolver {
            ResolverPolicy::System => None,
            ResolverPolicy::Bootstrap { resolver, timeout } => Some((resolver.clone(), *timeout)),
        }
    }

    #[cfg_attr(not(feature = "_http-client"), allow(dead_code))]
    pub(crate) fn has_custom_resolver(&self) -> bool {
        matches!(self.resolver, ResolverPolicy::Bootstrap { .. })
    }

    #[cfg_attr(not(feature = "_http-client"), allow(dead_code))]
    pub(crate) async fn resolve_host(&self, host: &str, port: u16) -> Result<IpAddr> {
        self.resolver.resolve_host(host, port).await
    }

    fn with_proxy(mut self, proxy: ProxyPolicy) -> Self {
        self.proxy = proxy;
        self
    }
}

impl Default for OutboundPolicy {
    fn default() -> Self {
        Self::system(None)
    }
}

#[derive(Debug, Clone)]
enum ResolverPolicy {
    System,
    Bootstrap {
        resolver: Arc<NameResolver>,
        #[cfg_attr(not(feature = "_http-client"), allow(dead_code))]
        timeout: Duration,
    },
}

impl ResolverPolicy {
    #[cfg_attr(not(feature = "_http-client"), allow(dead_code))]
    async fn resolve_host(&self, host: &str, port: u16) -> Result<IpAddr> {
        match self {
            Self::System => resolve_system(host, port).await,
            Self::Bootstrap { resolver, timeout } => {
                resolver.resolve(host, QueryDeadline::new(*timeout)).await
            }
        }
    }
}

#[derive(Debug, Clone)]
enum ProxyPolicy {
    Direct,
    Socks5(Socks5Opt),
}

impl ProxyPolicy {
    fn from_socks5(socks5: Option<Socks5Opt>) -> Self {
        socks5.map_or(Self::Direct, Self::Socks5)
    }

    fn socks5(&self) -> Option<Socks5Opt> {
        match self {
            Self::Direct => None,
            Self::Socks5(socks5) => Some(socks5.clone()),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct OutboundRuntime {
    default: Option<String>,
    profiles: HashMap<String, OutboundPolicy>,
}

impl OutboundRuntime {
    pub(crate) fn from_config(config: &NetworkOutboundConfig) -> Result<Self> {
        let mut profiles = HashMap::new();
        for (name, profile) in &config.profiles {
            profiles.insert(name.clone(), policy_from_profile(name, profile)?);
        }
        if let Some(default) = config.default.as_deref()
            && !profiles.contains_key(default)
        {
            return Err(DnsError::config(format!(
                "network.outbound.default references unknown profile '{}'",
                default
            )));
        }
        Ok(Self {
            default: config.default.clone(),
            profiles,
        })
    }

    pub(crate) fn resolve_policy(
        &self,
        outbound_ref: Option<&str>,
        legacy_socks5: Option<Socks5Opt>,
    ) -> Result<OutboundPolicy> {
        let mut policy = match outbound_ref
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .or(self.default.as_deref())
        {
            Some(name) => self.profiles.get(name).cloned().ok_or_else(|| {
                DnsError::config(format!("unknown network outbound profile '{}'", name))
            })?,
            None => OutboundPolicy::system(None),
        };

        if legacy_socks5.is_some() {
            policy = policy.with_proxy(ProxyPolicy::from_socks5(legacy_socks5));
        }

        Ok(policy)
    }
}

fn policy_from_profile(name: &str, profile: &OutboundProfileConfig) -> Result<OutboundPolicy> {
    let proxy = proxy_from_profile(name, profile)?;
    let resolver = match &profile.resolver {
        Some(OutboundResolverConfig::Mode(mode)) if mode.trim().eq_ignore_ascii_case("system") => {
            ResolverPolicy::System
        }
        Some(OutboundResolverConfig::Mode(mode)) => {
            return Err(DnsError::config(format!(
                "network.outbound profile '{}' has invalid resolver mode '{}'",
                name, mode
            )));
        }
        Some(OutboundResolverConfig::Nameservers(config)) => {
            resolver_from_nameservers(name, config, &proxy, network_metrics::profile_scope(name))?
        }
        None => ResolverPolicy::System,
    };

    Ok(OutboundPolicy { resolver, proxy })
}

fn proxy_from_profile(name: &str, profile: &OutboundProfileConfig) -> Result<ProxyPolicy> {
    let proxy = match &profile.proxy {
        Some(OutboundProxyConfig::Mode(mode))
            if mode.trim().eq_ignore_ascii_case("none")
                || mode.trim().eq_ignore_ascii_case("direct") =>
        {
            ProxyPolicy::Direct
        }
        Some(OutboundProxyConfig::Mode(mode)) => {
            return Err(DnsError::config(format!(
                "network.outbound profile '{}' has invalid proxy mode '{}'",
                name, mode
            )));
        }
        Some(OutboundProxyConfig::Socks5 { socks5 }) => {
            ProxyPolicy::Socks5(parse_socks5_opt(socks5).ok_or_else(|| {
                DnsError::config(format!(
                    "network.outbound profile '{}' has invalid socks5 proxy '{}'",
                    name, socks5
                ))
            })?)
        }
        None => ProxyPolicy::Direct,
    };
    Ok(proxy)
}

fn resolver_from_nameservers(
    name: &str,
    config: &OutboundResolverDetailedConfig,
    profile_proxy: &ProxyPolicy,
    metrics: Arc<NetworkProfileMetrics>,
) -> Result<ResolverPolicy> {
    let timeout = match config.timeout.as_deref() {
        Some(raw) => parse_simple_duration(raw).map_err(|err| {
            DnsError::config(format!(
                "network.outbound profile '{}' resolver.timeout is invalid: {}",
                name, err
            ))
        })?,
        None => DEFAULT_BOOTSTRAP_TIMEOUT,
    };
    let use_profile_proxy = matches!(config.proxy, Some(OutboundResolverProxyConfig::Profile));
    if use_profile_proxy && profile_proxy.socks5().is_none() {
        return Err(DnsError::config(format!(
            "network.outbound profile '{}' resolver.proxy profile requires a socks5 proxy",
            name
        )));
    }
    let socks5 = use_profile_proxy.then(|| profile_proxy.socks5()).flatten();
    let nameservers = config
        .nameservers
        .iter()
        .map(|nameserver| nameserver_config(nameserver, timeout, socks5.clone()))
        .collect::<Result<Vec<_>>>()?;
    Ok(ResolverPolicy::Bootstrap {
        resolver: Arc::new(NameResolver::from_nameserver_configs_with_metrics(
            nameservers,
            config.ip_version,
            metrics,
        )?),
        timeout,
    })
}

fn nameserver_config(
    nameserver: &OutboundNameserverConfig,
    timeout: Duration,
    socks5: Option<Socks5Opt>,
) -> Result<NameserverConfig> {
    NameserverConfig::new(
        nameserver.addr.clone(),
        nameserver.dial_addr,
        timeout,
        socks5,
    )
}

#[cfg_attr(not(feature = "_http-client"), allow(dead_code))]
async fn resolve_system(host: &str, port: u16) -> Result<IpAddr> {
    let mut addrs = tokio::net::lookup_host((host, port)).await.map_err(|err| {
        DnsError::protocol(format!(
            "Async DNS resolution failed for '{}': {}",
            host, err
        ))
    })?;
    addrs.next().map(|addr| addr.ip()).ok_or_else(|| {
        DnsError::protocol(format!("Async DNS returned no addresses for '{}'", host))
    })
}

fn global_slot() -> &'static Mutex<Arc<OutboundRuntime>> {
    static GLOBAL: OnceLock<Mutex<Arc<OutboundRuntime>>> = OnceLock::new();
    GLOBAL.get_or_init(|| Mutex::new(Arc::new(OutboundRuntime::default())))
}

pub(crate) fn install_global(config: &NetworkOutboundConfig) -> Result<()> {
    let runtime = Arc::new(OutboundRuntime::from_config(config)?);
    *global_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = runtime;
    Ok(())
}

pub(crate) fn restore_global(runtime: Arc<OutboundRuntime>) {
    *global_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = runtime;
}

pub(crate) fn clear_global() {
    *global_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(OutboundRuntime::default());
}

#[cfg(test)]
pub(crate) fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
pub(crate) struct TestGlobalGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Arc<OutboundRuntime>,
}

#[cfg(test)]
impl TestGlobalGuard {
    pub(crate) fn clean() -> Self {
        let lock = test_lock()
            .lock()
            .expect("outbound test lock should not be poisoned");
        let previous = global();
        clear_global();
        Self {
            _lock: lock,
            previous,
        }
    }
}

#[cfg(test)]
impl Drop for TestGlobalGuard {
    fn drop(&mut self) {
        restore_global(self.previous.clone());
    }
}

pub(crate) fn global() -> Arc<OutboundRuntime> {
    global_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::{NetworkOutboundConfig, OutboundResolverDetailedConfig};

    #[test]
    fn test_resolve_policy_defaults_to_direct_system() {
        let runtime = OutboundRuntime::default();
        let policy = runtime
            .resolve_policy(None, None)
            .expect("default policy should resolve");
        assert!(policy.proxy().is_none());
    }

    #[test]
    fn test_resolve_policy_uses_named_profile() {
        let config = NetworkOutboundConfig {
            default: None,
            profiles: HashMap::from([(
                "remote".to_string(),
                OutboundProfileConfig {
                    resolver: Some(OutboundResolverConfig::Nameservers(
                        OutboundResolverDetailedConfig {
                            nameservers: vec![OutboundNameserverConfig {
                                addr: "1.1.1.1:53".to_string(),
                                dial_addr: None,
                            }],
                            ip_version: Some(4),
                            timeout: None,
                            proxy: None,
                        },
                    )),
                    proxy: Some(OutboundProxyConfig::Socks5 {
                        socks5: "127.0.0.1:1080".to_string(),
                    }),
                },
            )]),
        };
        let runtime = OutboundRuntime::from_config(&config).expect("outbound config should parse");
        let policy = runtime
            .resolve_policy(Some("remote"), None)
            .expect("profile should resolve");
        assert!(policy.proxy().is_some());
        let (resolver, _) = policy
            .resolver()
            .expect("profile resolver should be configured");
        assert_eq!(resolver.profile(), "remote");
    }

    #[test]
    fn test_resolve_policy_default_keeps_profile_metric_label() {
        let config = NetworkOutboundConfig {
            default: Some("remote".to_string()),
            profiles: HashMap::from([(
                "remote".to_string(),
                OutboundProfileConfig {
                    resolver: Some(OutboundResolverConfig::Nameservers(
                        OutboundResolverDetailedConfig {
                            nameservers: vec![OutboundNameserverConfig {
                                addr: "1.1.1.1:53".to_string(),
                                dial_addr: None,
                            }],
                            ip_version: Some(4),
                            timeout: None,
                            proxy: None,
                        },
                    )),
                    proxy: None,
                },
            )]),
        };
        let runtime = OutboundRuntime::from_config(&config).expect("outbound config should parse");
        let policy = runtime
            .resolve_policy(None, None)
            .expect("default profile should resolve");

        let (resolver, _) = policy
            .resolver()
            .expect("default profile resolver should be configured");
        assert_eq!(resolver.profile(), "remote");
    }

    #[test]
    fn test_restore_global_reinstalls_previous_runtime() {
        clear_global();
        let first = NetworkOutboundConfig {
            default: Some("first".to_string()),
            profiles: HashMap::from([(
                "first".to_string(),
                OutboundProfileConfig {
                    resolver: None,
                    proxy: None,
                },
            )]),
        };
        let second = NetworkOutboundConfig {
            default: Some("second".to_string()),
            profiles: HashMap::from([(
                "second".to_string(),
                OutboundProfileConfig {
                    resolver: None,
                    proxy: None,
                },
            )]),
        };

        install_global(&first).expect("first outbound runtime should install");
        let snapshot = global();
        install_global(&second).expect("second outbound runtime should install");
        restore_global(snapshot);

        assert!(global().resolve_policy(Some("first"), None).is_ok());
        assert!(global().resolve_policy(Some("second"), None).is_err());
        clear_global();
    }
}
