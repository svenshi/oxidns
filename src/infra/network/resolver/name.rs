// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolver facade used by outbound clients and upstream bootstrap.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use rand::random;
use tracing::{debug, error, info, warn};

use super::cache::{ResolveEntry, ResolvedIp};
use super::client::{NameserverClient, build_clients};
use super::endpoint::NameserverConfig;
use super::query::{ResolvedAnswer, select_answer};
use crate::infra::error::{DnsError, Result};
use crate::infra::network::deadline::QueryDeadline;
use crate::infra::network::metrics::{
    self as network_metrics, NetworkProfileMetrics, OUTBOUND_PROFILE_LOCAL,
};
use crate::proto::{Message, Name, RecordType};

const MAX_RESOLVER_ENTRIES: usize = 4096;

/// Shared resolver backed by one or more DNS nameserver endpoints.
#[derive(Debug)]
pub(crate) struct NameResolver {
    clients: Vec<Arc<dyn NameserverClient>>,
    ip_version: Option<u8>,
    profile: String,
    metrics: Arc<NetworkProfileMetrics>,
    entries: Mutex<HashMap<String, Arc<ResolveEntry>>>,
}

impl NameResolver {
    pub(crate) fn new(servers: Vec<String>, ip_version: Option<u8>) -> Result<Self> {
        if servers.is_empty() {
            return Err(DnsError::config(
                "name resolver requires at least one server",
            ));
        }
        let nameservers = servers
            .into_iter()
            .map(|server| NameserverConfig::legacy_bootstrap(server.as_str()))
            .collect::<Result<Vec<_>>>()?;
        Self::from_nameserver_configs(nameservers, ip_version)
    }

    pub(crate) fn from_nameserver_configs(
        nameservers: Vec<NameserverConfig>,
        ip_version: Option<u8>,
    ) -> Result<Self> {
        Self::from_nameserver_configs_with_metrics(
            nameservers,
            ip_version,
            network_metrics::profile_scope(OUTBOUND_PROFILE_LOCAL),
        )
    }

    pub(crate) fn from_nameserver_configs_with_metrics(
        nameservers: Vec<NameserverConfig>,
        ip_version: Option<u8>,
        metrics: Arc<NetworkProfileMetrics>,
    ) -> Result<Self> {
        if nameservers.is_empty() {
            return Err(DnsError::config(
                "name resolver requires at least one server",
            ));
        }
        Ok(Self::from_clients_with_metrics(
            build_clients(nameservers)?,
            ip_version,
            metrics,
        ))
    }

    #[cfg(test)]
    fn from_clients(clients: Vec<Arc<dyn NameserverClient>>, ip_version: Option<u8>) -> Self {
        Self::from_clients_with_metrics(
            clients,
            ip_version,
            network_metrics::profile_scope(OUTBOUND_PROFILE_LOCAL),
        )
    }

    fn from_clients_with_metrics(
        clients: Vec<Arc<dyn NameserverClient>>,
        ip_version: Option<u8>,
        metrics: Arc<NetworkProfileMetrics>,
    ) -> Self {
        Self {
            clients,
            ip_version,
            profile: metrics.outbound_profile().to_string(),
            metrics,
            entries: Mutex::new(HashMap::new()),
        }
    }

    #[inline]
    pub(crate) async fn resolve(&self, host: &str, deadline: QueryDeadline) -> Result<IpAddr> {
        self.resolve_with_expiry(host, deadline)
            .await
            .map(|resolved| resolved.ip)
    }

    #[inline]
    pub(crate) async fn resolve_with_expiry(
        &self,
        host: &str,
        deadline: QueryDeadline,
    ) -> Result<ResolvedIp> {
        let domain = resolver_domain(host);
        let entry = match self.entry_for(domain) {
            Ok(entry) => entry,
            Err(err) => {
                network_metrics::resolver_error(self.metrics());
                return Err(err);
            }
        };
        entry
            .resolve_with(deadline, |request, query_name, deadline| {
                self.query_nameservers(request, query_name, deadline)
            })
            .await
    }

    #[cfg(test)]
    pub(crate) fn clear_entries_for_test(&self) {
        self.entries
            .lock()
            .expect("resolver entries lock should not be poisoned")
            .clear();
    }

    #[inline]
    pub(crate) fn profile(&self) -> &str {
        self.profile.as_str()
    }

    #[inline]
    pub(crate) fn metrics(&self) -> &NetworkProfileMetrics {
        debug_assert_eq!(self.profile(), self.metrics.outbound_profile());
        self.metrics.as_ref()
    }

    fn entry_for(&self, domain: String) -> Result<Arc<ResolveEntry>> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = entries.get(&domain) {
            entry.touch();
            return Ok(entry.clone());
        }
        prune_entries(&mut entries);
        if entries.len() >= MAX_RESOLVER_ENTRIES {
            return Err(DnsError::protocol(format!(
                "resolver cache entry limit exceeded ({MAX_RESOLVER_ENTRIES})"
            )));
        }
        let entry = Arc::new(ResolveEntry::new(
            domain.clone(),
            self.ip_version,
            self.metrics.clone(),
        )?);
        entries.insert(domain, entry.clone());
        Ok(entry)
    }

    async fn query_nameservers(
        &self,
        request: Message,
        query_name: Name,
        deadline: QueryDeadline,
    ) -> Result<ResolvedAnswer> {
        let mut last_error = None;

        for client in &self.clients {
            let mut message = request.clone();
            message.set_id(random());
            match client.query(message, deadline).await {
                Ok(response) => {
                    if let Some(answer) =
                        select_answer(response.answers(), &query_name, self.expected_record_type())
                    {
                        info!(
                            domain = %query_name.to_fqdn(),
                            server = %client.label(),
                            ip = %answer.ip,
                            ttl_seconds = answer.ttl_seconds,
                            record_type = ?answer.record_type,
                            "Resolver DNS resolution successful"
                        );
                        return Ok(answer);
                    }

                    warn!(
                        domain = %query_name.to_fqdn(),
                        server = %client.label(),
                        answer_count = response.answers().len(),
                        "No A/AAAA records found in resolver DNS response"
                    );
                    last_error = Some(DnsError::protocol(format!(
                        "No A/AAAA records found in resolver DNS response for '{}'",
                        query_name.to_fqdn()
                    )));
                }
                Err(err) => {
                    error!(
                        domain = %query_name.to_fqdn(),
                        server = %client.label(),
                        error = %err,
                        "Resolver DNS query failed"
                    );
                    last_error = Some(err);
                }
            }
        }

        let err = last_error.unwrap_or_else(|| {
            DnsError::protocol(format!(
                "Resolver DNS resolution failed for '{}'",
                query_name.to_fqdn()
            ))
        });
        debug!(domain = %query_name.to_fqdn(), error = %err, "Resolver query exhausted servers");
        Err(err)
    }

    fn expected_record_type(&self) -> RecordType {
        match self.ip_version {
            Some(6) => RecordType::AAAA,
            _ => RecordType::A,
        }
    }
}

fn prune_entries(entries: &mut HashMap<String, Arc<ResolveEntry>>) {
    if entries.len() < MAX_RESOLVER_ENTRIES {
        return;
    }

    entries.retain(|_, entry| !(Arc::strong_count(entry) == 1 && entry.is_expired_hint()));

    while entries.len() >= MAX_RESOLVER_ENTRIES {
        let Some(evict_key) = entries
            .iter()
            .filter(|(_, entry)| Arc::strong_count(entry) == 1)
            .min_by_key(|(_, entry)| entry.last_accessed_at())
            .map(|(domain, _)| domain.clone())
        else {
            break;
        };
        entries.remove(&evict_key);
    }
}

fn resolver_domain(host: &str) -> String {
    if host.ends_with('.') {
        host.to_string()
    } else {
        format!("{host}.")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future::pending;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::time::Duration;

    use async_trait::async_trait;
    use tokio::sync::oneshot;

    use super::*;
    use crate::infra::clock::AppClock;
    use crate::infra::network::metrics as network_metrics;
    use crate::infra::network::resolver::ResolvedIp;
    use crate::proto::rdata::A;
    use crate::proto::{RData, Record};

    #[derive(Debug)]
    enum FakeOutcome {
        Response(Message),
        Error(&'static str),
    }

    #[derive(Debug)]
    struct FakeClient {
        label: String,
        outcomes: Mutex<VecDeque<FakeOutcome>>,
        count: AtomicUsize,
    }

    impl FakeClient {
        fn new(label: &str, outcomes: Vec<FakeOutcome>) -> Self {
            Self {
                label: label.to_string(),
                outcomes: Mutex::new(VecDeque::from(outcomes)),
                count: AtomicUsize::new(0),
            }
        }

        fn count(&self) -> usize {
            self.count.load(AtomicOrdering::Relaxed)
        }
    }

    #[async_trait]
    impl NameserverClient for FakeClient {
        async fn query(&self, _request: Message, _deadline: QueryDeadline) -> Result<Message> {
            self.count.fetch_add(1, AtomicOrdering::Relaxed);
            match self
                .outcomes
                .lock()
                .expect("outcomes lock should not be poisoned")
                .pop_front()
            {
                Some(FakeOutcome::Response(response)) => Ok(response),
                Some(FakeOutcome::Error(message)) => Err(DnsError::protocol(message)),
                None => Err(DnsError::protocol("no fake response configured")),
            }
        }

        fn label(&self) -> &str {
            self.label.as_str()
        }
    }

    #[derive(Debug)]
    struct SlowClient {
        label: String,
        response: Message,
        count: AtomicUsize,
    }

    #[async_trait]
    impl NameserverClient for SlowClient {
        async fn query(&self, _request: Message, _deadline: QueryDeadline) -> Result<Message> {
            self.count.fetch_add(1, AtomicOrdering::Relaxed);
            tokio::time::sleep(Duration::from_millis(30)).await;
            Ok(self.response.clone())
        }

        fn label(&self) -> &str {
            self.label.as_str()
        }
    }

    #[derive(Debug)]
    struct BlockingThenClient {
        started: Mutex<Option<oneshot::Sender<()>>>,
        response: Message,
        count: AtomicUsize,
    }

    #[async_trait]
    impl NameserverClient for BlockingThenClient {
        async fn query(&self, _request: Message, _deadline: QueryDeadline) -> Result<Message> {
            let count = self.count.fetch_add(1, AtomicOrdering::Relaxed);
            if count == 0 {
                if let Some(started) = self
                    .started
                    .lock()
                    .expect("started lock should not be poisoned")
                    .take()
                {
                    let _ = started.send(());
                }
                pending::<Result<Message>>().await
            } else {
                Ok(self.response.clone())
            }
        }

        fn label(&self) -> &str {
            "blocking-then"
        }
    }

    fn start_clock() {
        AppClock::start();
    }

    fn answer_response(name: &str, ttl: u32, ip: IpAddr) -> Message {
        let name = Name::from_ascii(name).expect("answer name should parse");
        let mut message = Message::new();
        let IpAddr::V4(ip) = ip else {
            panic!("test answer should be IPv4");
        };
        message.add_answer(Record::from_rdata(name, ttl, RData::A(A(ip))));
        message
    }

    #[tokio::test]
    async fn test_resolver_falls_back_to_next_nameserver() {
        start_clock();
        let first = Arc::new(FakeClient::new("first", vec![FakeOutcome::Error("boom")]));
        let second = Arc::new(FakeClient::new(
            "second",
            vec![FakeOutcome::Response(answer_response(
                "example.com.",
                60,
                IpAddr::V4(Ipv4Addr::new(203, 0, 113, 53)),
            ))],
        ));
        let resolver = NameResolver::from_clients(vec![first.clone(), second.clone()], None);

        let ip = resolver
            .resolve(
                "example.com",
                QueryDeadline::new(Duration::from_millis(200)),
            )
            .await
            .expect("second nameserver should resolve");

        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 53)));
        assert_eq!(first.count(), 1);
        assert_eq!(second.count(), 1);
    }

    #[tokio::test]
    async fn test_concurrent_resolve_singleflights_same_domain() {
        start_clock();
        let client = Arc::new(SlowClient {
            label: "slow".to_string(),
            response: answer_response(
                "example.com.",
                60,
                IpAddr::V4(Ipv4Addr::new(203, 0, 113, 53)),
            ),
            count: AtomicUsize::new(0),
        });
        let resolver = Arc::new(NameResolver::from_clients(vec![client.clone()], None));

        let mut handles = Vec::new();
        for _ in 0..5 {
            let resolver = resolver.clone();
            handles.push(tokio::spawn(async move {
                resolver
                    .resolve(
                        "example.com",
                        QueryDeadline::new(Duration::from_millis(500)),
                    )
                    .await
            }));
        }

        for handle in handles {
            let ip = handle
                .await
                .expect("task should complete")
                .expect("resolve should succeed");
            assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 53)));
        }
        assert_eq!(client.count.load(AtomicOrdering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_expired_cache_refreshes() {
        start_clock();
        let client = Arc::new(FakeClient::new(
            "fake",
            vec![
                FakeOutcome::Response(answer_response(
                    "example.com.",
                    60,
                    IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
                )),
                FakeOutcome::Response(answer_response(
                    "example.com.",
                    60,
                    IpAddr::V4(Ipv4Addr::new(203, 0, 113, 2)),
                )),
            ],
        ));
        let resolver = NameResolver::from_clients(vec![client.clone()], None);

        let first = resolver
            .resolve(
                "example.com",
                QueryDeadline::new(Duration::from_millis(200)),
            )
            .await
            .expect("first resolve should succeed");
        let entry = resolver
            .entry_for("example.com.".to_string())
            .expect("entry should exist");
        *entry.cache.write().await = Some(ResolvedIp {
            ip: first,
            expires_at_ms: 0,
        });
        let second = resolver
            .resolve(
                "example.com",
                QueryDeadline::new(Duration::from_millis(200)),
            )
            .await
            .expect("second resolve should refresh");

        assert_eq!(first, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)));
        assert_eq!(second, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 2)));
        assert_eq!(client.count(), 2);
    }

    #[tokio::test]
    async fn resolver_metrics_record_hit_miss_refresh_and_error() {
        start_clock();
        let profile = network_metrics::profile_scope("remote");
        let before = network_metrics::snapshot_for_profile_for_tests("remote");
        let client = Arc::new(FakeClient::new(
            "fake",
            vec![
                FakeOutcome::Response(answer_response(
                    "example.com.",
                    60,
                    IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
                )),
                FakeOutcome::Error("boom"),
            ],
        ));
        let resolver = NameResolver::from_clients_with_metrics(vec![client], None, profile);

        let first = resolver
            .resolve(
                "example.com",
                QueryDeadline::new(Duration::from_millis(200)),
            )
            .await
            .expect("first resolve should refresh");
        let second = resolver
            .resolve(
                "example.com",
                QueryDeadline::new(Duration::from_millis(200)),
            )
            .await
            .expect("second resolve should hit cache");
        let err = resolver
            .resolve(
                "failed.example",
                QueryDeadline::new(Duration::from_millis(200)),
            )
            .await
            .expect_err("resolver refresh failure should be returned");

        assert_eq!(first, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)));
        assert_eq!(second, first);
        assert!(err.to_string().contains("boom"));

        let after = network_metrics::snapshot_for_profile_for_tests("remote");
        assert!(
            after.resolver_cache_hit_total > before.resolver_cache_hit_total,
            "expected resolver cache hit metric to increase: before={before:?}, after={after:?}"
        );
        assert!(
            after.resolver_cache_miss_total >= before.resolver_cache_miss_total + 2,
            "expected resolver cache miss metric to increase: before={before:?}, after={after:?}"
        );
        assert!(
            after.resolver_refresh_total >= before.resolver_refresh_total + 2,
            "expected resolver refresh metric to increase: before={before:?}, after={after:?}"
        );
        assert!(
            after.resolver_error_total > before.resolver_error_total,
            "expected resolver error metric to increase: before={before:?}, after={after:?}"
        );
    }

    #[test]
    fn test_resolver_entry_map_is_bounded() {
        start_clock();
        let client = Arc::new(FakeClient::new("fake", vec![]));
        let resolver = NameResolver::from_clients(vec![client], None);

        for index in 0..(MAX_RESOLVER_ENTRIES + 10) {
            let _ = resolver
                .entry_for(format!("host{index}.example."))
                .expect("entry should be created");
        }

        let len = resolver
            .entries
            .lock()
            .expect("entries lock should not be poisoned")
            .len();
        assert!(len <= MAX_RESOLVER_ENTRIES, "entry map grew to {len}");
    }

    #[test]
    fn test_resolver_rejects_new_entries_when_cache_cap_is_active() {
        start_clock();
        let client = Arc::new(FakeClient::new("fake", vec![]));
        let resolver = NameResolver::from_clients(vec![client], None);
        let mut active_entries = Vec::new();

        for index in 0..MAX_RESOLVER_ENTRIES {
            active_entries.push(
                resolver
                    .entry_for(format!("active{index}.example."))
                    .expect("entry should be created"),
            );
        }

        let err = resolver
            .entry_for("overflow.example.".to_string())
            .expect_err("active cache cap should reject new domains");

        assert!(err.to_string().contains("entry limit exceeded"), "{err}");
        assert_eq!(
            resolver
                .entries
                .lock()
                .expect("entries lock should not be poisoned")
                .len(),
            MAX_RESOLVER_ENTRIES
        );
        drop(active_entries);
    }

    #[tokio::test]
    async fn test_canceled_refresh_releases_singleflight_lock() {
        start_clock();
        let (started_tx, started_rx) = oneshot::channel();
        let client = Arc::new(BlockingThenClient {
            started: Mutex::new(Some(started_tx)),
            response: answer_response(
                "example.com.",
                60,
                IpAddr::V4(Ipv4Addr::new(203, 0, 113, 53)),
            ),
            count: AtomicUsize::new(0),
        });
        let resolver = Arc::new(NameResolver::from_clients(vec![client], None));

        let first = resolver.clone();
        let handle = tokio::spawn(async move {
            first
                .resolve("example.com", QueryDeadline::new(Duration::from_secs(5)))
                .await
        });

        started_rx.await.expect("first query should start");
        handle.abort();
        assert!(
            handle
                .await
                .expect_err("resolve task should be cancelled")
                .is_cancelled()
        );

        let ip = resolver
            .resolve(
                "example.com",
                QueryDeadline::new(Duration::from_millis(200)),
            )
            .await
            .expect("second query should acquire refresh lock");

        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 53)));
    }
}
