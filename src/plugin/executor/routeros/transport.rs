// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared RouterOS transport for MikroTik-backed executor plugins.

use std::collections::HashMap;
use std::fmt::{Debug, Display, Formatter};
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mikrotik_rs::{Command, Event, MikrotikDevice, TrapCategory};
use rustls::pki_types::ServerName;
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::infra::error::{DnsError, Result};
use crate::infra::network::tls_config::secure_client_config_with_additional_roots;

const BACKOFF_BASE_SECS: u64 = 1;
const BACKOFF_MAX_SECS: u64 = 30;

/// RouterOS API operation timeouts.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct RouterOsTimeouts {
    pub(crate) connect: Duration,
    pub(crate) send: Duration,
    pub(crate) receive: Duration,
}

impl RouterOsTimeouts {
    pub(crate) fn from_secs(connect: u64, send: u64, receive: u64) -> Self {
        Self {
            connect: Duration::from_secs(connect),
            send: Duration::from_secs(send),
            receive: Duration::from_secs(receive),
        }
    }
}

impl Default for RouterOsTimeouts {
    fn default() -> Self {
        Self::from_secs(5, 5, 5)
    }
}

/// User-facing nested TLS configuration. Presence enables API-SSL.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct RouterOsTlsArgs {
    pub(crate) insecure: Option<bool>,
    pub(crate) server_name: Option<String>,
    pub(crate) ca: Option<String>,
}

#[derive(Clone)]
enum RouterOsTlsMode {
    Insecure,
    Secure {
        config: Arc<rustls::ClientConfig>,
        server_name: ServerName<'static>,
        ca_path: Option<String>,
    },
}

impl Debug for RouterOsTlsMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Insecure => f.write_str("Insecure"),
            Self::Secure {
                server_name,
                ca_path,
                ..
            } => f
                .debug_struct("Secure")
                .field("server_name", server_name)
                .field("ca_path", ca_path)
                .finish(),
        }
    }
}

/// Connection settings consumed by [`RouterOsTransport`].
#[derive(Clone)]
pub(crate) struct RouterOsConnectionConfig {
    pub(crate) address: String,
    pub(crate) username: String,
    password: String,
    pub(crate) timeouts: RouterOsTimeouts,
    tls: Option<RouterOsTlsMode>,
}

impl Debug for RouterOsConnectionConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouterOsConnectionConfig")
            .field("address", &self.address)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .field("timeouts", &self.timeouts)
            .field("tls", &self.tls)
            .finish()
    }
}

impl RouterOsConnectionConfig {
    pub(crate) fn new(
        address: String,
        username: String,
        password: String,
        timeouts: RouterOsTimeouts,
        tls: Option<RouterOsTlsArgs>,
    ) -> Result<Self> {
        let tls = tls.map(|args| build_tls_mode(&address, args)).transpose()?;
        Ok(Self {
            address,
            username,
            password,
            timeouts,
            tls,
        })
    }

    #[cfg(test)]
    fn plaintext_for_test() -> Self {
        Self {
            address: "127.0.0.1:8728".to_string(),
            username: "api".to_string(),
            password: "secret".to_string(),
            timeouts: RouterOsTimeouts::from_secs(1, 1, 1),
            tls: None,
        }
    }
}

fn build_tls_mode(address: &str, args: RouterOsTlsArgs) -> Result<RouterOsTlsMode> {
    let insecure = args.insecure.unwrap_or(false);
    let ca = args
        .ca
        .map(|value| value.trim().to_string())
        .filter(|v| !v.is_empty());
    let explicit_server_name = args
        .server_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if insecure {
        if ca.is_some() {
            return Err(DnsError::plugin(
                "RouterOS tls.insecure cannot be combined with tls.ca",
            ));
        }
        return Ok(RouterOsTlsMode::Insecure);
    }

    let server_name_raw = explicit_server_name
        .or_else(|| infer_server_name(address).map(str::to_string))
        .ok_or_else(|| DnsError::plugin("RouterOS TLS could not infer server name from address"))?;
    let server_name = ServerName::try_from(server_name_raw.clone()).map_err(|error| {
        DnsError::plugin(format!(
            "RouterOS TLS server name '{server_name_raw}' is invalid: {error}"
        ))
    })?;

    let config = if let Some(path) = ca.as_deref() {
        let file = File::open(path).map_err(|error| {
            DnsError::plugin(format!(
                "failed to open RouterOS TLS CA file '{path}': {error}"
            ))
        })?;
        let mut reader = BufReader::new(file);
        let certificates = rustls_pemfile::certs(&mut reader)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| {
                DnsError::plugin(format!(
                    "failed to parse RouterOS TLS CA file '{path}': {error}"
                ))
            })?;
        if certificates.is_empty() {
            return Err(DnsError::plugin(format!(
                "RouterOS TLS CA file '{path}' contains no certificates"
            )));
        }
        secure_client_config_with_additional_roots(certificates).map_err(|error| {
            DnsError::plugin(format!(
                "failed to load RouterOS TLS CA file '{path}': {error}"
            ))
        })?
    } else {
        crate::infra::network::tls_config::secure_client_config()
    };

    Ok(RouterOsTlsMode::Secure {
        config: Arc::new(config),
        server_name,
        ca_path: ca,
    })
}

fn infer_server_name(address: &str) -> Option<&str> {
    let address = address.trim();
    if let Some(rest) = address.strip_prefix('[') {
        return rest.split_once(']').map(|(host, _)| host);
    }
    address
        .rsplit_once(':')
        .map(|(host, _)| host)
        .filter(|host| !host.is_empty())
}

/// RouterOS trap message emitted when an insert targets a value the router
/// already holds for the same object.
const DUPLICATE_ENTRY_TRAP_MESSAGE: &str = "already have such entry";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RouterOsErrorKind {
    Backoff,
    Connect,
    ConnectTimeout,
    Send,
    SendTimeout,
    ReceiveTimeout,
    ChannelClosed,
    Fatal,
    Trap,
}

#[derive(Debug)]
pub(crate) struct RouterOsError {
    pub(crate) kind: RouterOsErrorKind,
    pub(crate) action: String,
    pub(crate) message: String,
    pub(crate) trap_category: Option<TrapCategory>,
}

impl RouterOsError {
    fn new(kind: RouterOsErrorKind, action: &str, message: impl Into<String>) -> Self {
        Self {
            kind,
            action: action.to_string(),
            message: message.into(),
            trap_category: None,
        }
    }

    fn trap(action: &str, category: Option<TrapCategory>, message: String) -> Self {
        Self {
            kind: RouterOsErrorKind::Trap,
            action: action.to_string(),
            message,
            trap_category: category,
        }
    }

    pub(crate) fn is_missing_item(&self) -> bool {
        self.kind == RouterOsErrorKind::Trap
            && self.trap_category == Some(TrapCategory::MissingItemOrCommand)
    }

    /// Whether the trap reports that an equivalent row already exists.
    ///
    /// RouterOS refuses a plain insert when the target already holds the same
    /// value and reports it as a `failure:` message rather than a dedicated
    /// category, so the message text is the only stable signal. Callers that
    /// map a desired state onto RouterOS objects must treat this as an
    /// idempotent outcome instead of a failure.
    pub(crate) fn is_duplicate_entry(&self) -> bool {
        self.kind == RouterOsErrorKind::Trap
            && self
                .message
                .to_ascii_lowercase()
                .contains(DUPLICATE_ENTRY_TRAP_MESSAGE)
    }
}

impl Display for RouterOsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "RouterOS {} failed: {}", self.action, self.message)
    }
}

impl std::error::Error for RouterOsError {}

impl From<RouterOsError> for DnsError {
    fn from(error: RouterOsError) -> Self {
        DnsError::plugin(error.to_string())
    }
}

pub(crate) type RouterOsResult<T> = std::result::Result<T, RouterOsError>;

/// Lock-free counters plus retry state exposed to plugin metrics and workers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RouterOsTransportSnapshot {
    pub(crate) reconnect_total: u64,
    pub(crate) backoff_total: u64,
    pub(crate) connect_attempt_total: u64,
    pub(crate) degraded: bool,
    pub(crate) retry_after: Option<Duration>,
}

#[derive(Debug)]
struct ConnectionState {
    generation: u64,
    device: Option<MikrotikDevice>,
    consecutive_failures: u32,
    retry_at: Option<Instant>,
}

#[derive(Clone)]
pub(crate) struct RouterOsTransport {
    config: Arc<RouterOsConnectionConfig>,
    state: Arc<Mutex<ConnectionState>>,
    shutdown_cleanup: Arc<AtomicBool>,
    reconnect_total: Arc<AtomicU64>,
    backoff_total: Arc<AtomicU64>,
    connect_attempt_total: Arc<AtomicU64>,
}

impl Debug for RouterOsTransport {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouterOsTransport")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl RouterOsTransport {
    pub(crate) fn new(config: RouterOsConnectionConfig) -> Self {
        Self {
            config: Arc::new(config),
            state: Arc::new(Mutex::new(ConnectionState {
                generation: 0,
                device: None,
                consecutive_failures: 0,
                retry_at: None,
            })),
            shutdown_cleanup: Arc::new(AtomicBool::new(false)),
            reconnect_total: Arc::new(AtomicU64::new(0)),
            backoff_total: Arc::new(AtomicU64::new(0)),
            connect_attempt_total: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Make all following commands bypass reconnect backoff during ownership
    /// cleanup. Individual connect/send/receive timeouts remain unchanged.
    pub(crate) fn begin_shutdown_cleanup(&self) {
        self.shutdown_cleanup.store(true, Ordering::Release);
    }

    pub(crate) async fn snapshot(&self) -> RouterOsTransportSnapshot {
        let state = self.state.lock().await;
        let now = Instant::now();
        RouterOsTransportSnapshot {
            reconnect_total: self.reconnect_total.load(Ordering::Relaxed),
            backoff_total: self.backoff_total.load(Ordering::Relaxed),
            connect_attempt_total: self.connect_attempt_total.load(Ordering::Relaxed),
            degraded: state.device.is_none() && state.consecutive_failures > 0,
            retry_after: state
                .retry_at
                .map(|retry_at| retry_at.saturating_duration_since(now))
                .filter(|delay| !delay.is_zero()),
        }
    }

    pub(crate) async fn send_command(
        &self,
        action: &str,
        command: Command,
    ) -> RouterOsResult<RouterOsCommandStream> {
        let bypass_backoff = self.shutdown_cleanup.load(Ordering::Acquire);
        self.send_command_with_backoff(action, command, bypass_backoff)
            .await
    }

    /// Sends one command without waiting for the current reconnect backoff.
    ///
    /// This is reserved for bounded cleanup operations whose objects may have
    /// reached RouterOS before the response failed. A failed bypass attempt
    /// still records the connection failure and schedules the next backoff.
    #[cfg(any(feature = "plugin-ros-route", test))]
    pub(crate) async fn send_command_bypassing_backoff(
        &self,
        action: &str,
        command: Command,
    ) -> RouterOsResult<RouterOsCommandStream> {
        self.send_command_with_backoff(action, command, true).await
    }

    async fn send_command_with_backoff(
        &self,
        action: &str,
        command: Command,
        bypass_backoff: bool,
    ) -> RouterOsResult<RouterOsCommandStream> {
        let (device, generation) = self.get_or_connect(action, bypass_backoff).await?;
        match tokio::time::timeout(self.config.timeouts.send, device.send_command(command)).await {
            Ok(Ok(receiver)) => Ok(RouterOsCommandStream {
                action: action.to_string(),
                receiver,
                generation,
                transport: self.clone(),
                completed: false,
            }),
            Ok(Err(error)) => {
                self.record_failure(generation).await;
                Err(RouterOsError::new(
                    RouterOsErrorKind::Send,
                    action,
                    error.to_string(),
                ))
            }
            Err(_) => {
                self.record_failure(generation).await;
                Err(RouterOsError::new(
                    RouterOsErrorKind::SendTimeout,
                    action,
                    format!(
                        "send timeout after {}s",
                        self.config.timeouts.send.as_secs()
                    ),
                ))
            }
        }
    }

    async fn get_or_connect(
        &self,
        action: &str,
        bypass_backoff: bool,
    ) -> RouterOsResult<(MikrotikDevice, u64)> {
        let mut state = self.state.lock().await;
        if let Some(device) = state.device.as_ref() {
            return Ok((device.clone(), state.generation));
        }
        if !bypass_backoff
            && let Some(retry_at) = state.retry_at
            && retry_at > Instant::now()
        {
            return Err(RouterOsError::new(
                RouterOsErrorKind::Backoff,
                action,
                format!(
                    "reconnect delayed for {:?}",
                    retry_at.saturating_duration_since(Instant::now())
                ),
            ));
        }

        self.connect_attempt_total.fetch_add(1, Ordering::Relaxed);
        let connect = async {
            let builder = MikrotikDevice::builder(self.config.address.as_str()).credentials(
                self.config.username.as_str(),
                Some(self.config.password.as_str()),
            );
            match self.config.tls.as_ref() {
                None => builder.connect().await,
                Some(RouterOsTlsMode::Insecure) => builder.tls_insecure().connect().await,
                Some(RouterOsTlsMode::Secure {
                    config,
                    server_name,
                    ..
                }) => {
                    builder
                        .tls_config(config.clone(), server_name.clone())
                        .connect()
                        .await
                }
            }
        };
        let device = match tokio::time::timeout(self.config.timeouts.connect, connect).await {
            Ok(Ok(device)) => device,
            Ok(Err(error)) => {
                self.schedule_backoff(&mut state);
                return Err(RouterOsError::new(
                    RouterOsErrorKind::Connect,
                    action,
                    format!("connect to {} failed: {error}", self.config.address),
                ));
            }
            Err(_) => {
                self.schedule_backoff(&mut state);
                return Err(RouterOsError::new(
                    RouterOsErrorKind::ConnectTimeout,
                    action,
                    format!(
                        "connect to {} timed out after {}s",
                        self.config.address,
                        self.config.timeouts.connect.as_secs()
                    ),
                ));
            }
        };
        if state.generation > 0 {
            self.reconnect_total.fetch_add(1, Ordering::Relaxed);
        }
        state.generation = state.generation.wrapping_add(1);
        state.device = Some(device.clone());
        Ok((device, state.generation))
    }

    async fn record_failure(&self, generation: u64) {
        let mut state = self.state.lock().await;
        if state.generation == generation && state.device.take().is_some() {
            self.schedule_backoff(&mut state);
        }
    }

    async fn record_success(&self, generation: u64) {
        let mut state = self.state.lock().await;
        // A pipelined command can complete after another command from the same
        // connection generation has already failed and invalidated `device`.
        // That stale success must not clear the newly scheduled reconnect
        // backoff or the degraded state.
        if state.generation == generation && state.device.is_some() {
            state.consecutive_failures = 0;
            state.retry_at = None;
        }
    }

    fn schedule_backoff(&self, state: &mut ConnectionState) {
        schedule_backoff(state);
        self.backoff_total.fetch_add(1, Ordering::Relaxed);
    }
}

fn schedule_backoff(state: &mut ConnectionState) {
    state.consecutive_failures = state.consecutive_failures.saturating_add(1);
    let shift = state.consecutive_failures.saturating_sub(1).min(5);
    let seconds = BACKOFF_BASE_SECS
        .saturating_mul(1u64 << shift)
        .min(BACKOFF_MAX_SECS);
    let base_millis = seconds.saturating_mul(1_000);
    let jitter_span = base_millis / 5;
    let random = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::from(duration.subsec_nanos()))
        .unwrap_or(0);
    let jittered_millis = base_millis
        .saturating_sub(jitter_span)
        .saturating_add(random % jitter_span.saturating_mul(2).saturating_add(1));
    state.retry_at = Some(Instant::now() + Duration::from_millis(jittered_millis));
}

#[derive(Debug)]
pub(crate) enum RouterOsEvent {
    Reply(HashMap<String, Option<String>>),
    Complete,
}

pub(crate) struct RouterOsCommandStream {
    action: String,
    receiver: tokio::sync::mpsc::UnboundedReceiver<Event>,
    generation: u64,
    transport: RouterOsTransport,
    completed: bool,
}

impl RouterOsCommandStream {
    pub(crate) async fn next(&mut self) -> RouterOsResult<RouterOsEvent> {
        if self.completed {
            return Ok(RouterOsEvent::Complete);
        }
        let event = match tokio::time::timeout(
            self.transport.config.timeouts.receive,
            self.receiver.recv(),
        )
        .await
        {
            Ok(Some(event)) => event,
            Ok(None) => {
                self.transport.record_failure(self.generation).await;
                return Err(RouterOsError::new(
                    RouterOsErrorKind::ChannelClosed,
                    &self.action,
                    "response channel closed before a terminal event",
                ));
            }
            Err(_) => {
                self.transport.record_failure(self.generation).await;
                return Err(RouterOsError::new(
                    RouterOsErrorKind::ReceiveTimeout,
                    &self.action,
                    format!(
                        "receive timeout after {}s",
                        self.transport.config.timeouts.receive.as_secs()
                    ),
                ));
            }
        };

        match event {
            Event::Reply { response, .. } => Ok(RouterOsEvent::Reply(response.attributes)),
            Event::Done { .. } | Event::Empty { .. } => {
                self.completed = true;
                self.transport.record_success(self.generation).await;
                Ok(RouterOsEvent::Complete)
            }
            Event::Trap { response, .. } => Err(RouterOsError::trap(
                &self.action,
                response.category,
                response.message,
            )),
            Event::Fatal { reason } => {
                self.transport.record_failure(self.generation).await;
                Err(RouterOsError::new(
                    RouterOsErrorKind::Fatal,
                    &self.action,
                    reason,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use mikrotik_rs::{CommandBuilder, Tag, TrapResponse};

    use super::*;

    #[test]
    fn connection_debug_redacts_password() {
        let debug = format!("{:?}", RouterOsConnectionConfig::plaintext_for_test());
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn server_name_is_inferred_from_host_and_ipv6_addresses() {
        assert_eq!(
            infer_server_name("router.example:8729"),
            Some("router.example")
        );
        assert_eq!(infer_server_name("[2001:db8::1]:8729"), Some("2001:db8::1"));
    }

    #[test]
    fn insecure_tls_rejects_conflicting_verification_options() {
        let error = build_tls_mode(
            "router.example:8729",
            RouterOsTlsArgs {
                insecure: Some(true),
                ca: Some("router-ca.pem".to_string()),
                server_name: None,
            },
        )
        .expect_err("insecure and CA must conflict");
        assert!(error.to_string().contains("cannot be combined"));
    }

    #[test]
    fn insecure_tls_allows_unused_explicit_server_name() {
        let mode = build_tls_mode(
            "192.0.2.1:8729",
            RouterOsTlsArgs {
                insecure: Some(true),
                ca: None,
                server_name: Some("router.example".to_string()),
            },
        )
        .expect("explicit name is harmless in insecure mode");
        assert!(matches!(mode, RouterOsTlsMode::Insecure));
    }

    #[test]
    fn secure_tls_uses_inferred_or_explicit_server_name() {
        for (address, explicit, expected) in [
            ("router.example:8729", None, "router.example"),
            ("192.0.2.1:8729", Some("router.example"), "router.example"),
        ] {
            let mode = build_tls_mode(
                address,
                RouterOsTlsArgs {
                    insecure: Some(false),
                    ca: None,
                    server_name: explicit.map(str::to_string),
                },
            )
            .expect("secure TLS mode");
            let RouterOsTlsMode::Secure { server_name, .. } = mode else {
                panic!("expected secure TLS");
            };
            assert_eq!(server_name.to_str(), expected);
        }
    }

    #[test]
    fn backoff_is_exponential_and_capped() {
        let mut state = ConnectionState {
            generation: 0,
            device: None,
            consecutive_failures: 0,
            retry_at: None,
        };
        let now = Instant::now();
        for _ in 0..10 {
            schedule_backoff(&mut state);
        }
        let delay = state
            .retry_at
            .expect("retry")
            .saturating_duration_since(now);
        assert!(delay >= Duration::from_secs(24));
        assert!(delay <= Duration::from_secs(37));
    }

    #[tokio::test]
    async fn response_channel_close_before_terminal_event_is_an_error() {
        let transport = RouterOsTransport::new(RouterOsConnectionConfig::plaintext_for_test());
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        drop(sender);
        let mut stream = RouterOsCommandStream {
            action: "test command".to_string(),
            receiver,
            generation: 0,
            transport,
            completed: false,
        };

        let error = stream.next().await.expect_err("premature close must fail");
        assert_eq!(error.kind, RouterOsErrorKind::ChannelClosed);
    }

    #[tokio::test]
    async fn trap_preserves_connection_state_and_missing_item_category() {
        let transport = RouterOsTransport::new(RouterOsConnectionConfig::plaintext_for_test());
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        sender
            .send(Event::Trap {
                tag: Tag::default(),
                response: TrapResponse {
                    tag: Tag::default(),
                    category: Some(TrapCategory::MissingItemOrCommand),
                    message: "no such item".to_string(),
                },
            })
            .expect("trap event");
        let mut stream = RouterOsCommandStream {
            action: "delete".to_string(),
            receiver,
            generation: 0,
            transport: transport.clone(),
            completed: false,
        };

        let error = stream.next().await.expect_err("trap");

        assert_eq!(error.kind, RouterOsErrorKind::Trap);
        assert!(error.is_missing_item());
        assert_eq!(transport.snapshot().await.backoff_total, 0);
    }

    #[tokio::test]
    async fn duplicate_entry_trap_is_classified_without_a_category() {
        let transport = RouterOsTransport::new(RouterOsConnectionConfig::plaintext_for_test());
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        sender
            .send(Event::Trap {
                tag: Tag::default(),
                response: TrapResponse {
                    tag: Tag::default(),
                    category: None,
                    message: "failure: already have such entry".to_string(),
                },
            })
            .expect("trap event");
        let mut stream = RouterOsCommandStream {
            action: "add address-list entry".to_string(),
            receiver,
            generation: 0,
            transport,
            completed: false,
        };

        let error = stream.next().await.expect_err("trap");

        assert!(error.is_duplicate_entry());
        assert!(!error.is_missing_item());
    }

    #[tokio::test]
    async fn unrelated_trap_is_not_a_duplicate_entry() {
        let transport = RouterOsTransport::new(RouterOsConnectionConfig::plaintext_for_test());
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        sender
            .send(Event::Trap {
                tag: Tag::default(),
                response: TrapResponse {
                    tag: Tag::default(),
                    category: Some(TrapCategory::ArgumentValueFailure),
                    message: "input does not match any value of interface".to_string(),
                },
            })
            .expect("trap event");
        let mut stream = RouterOsCommandStream {
            action: "add address-list entry".to_string(),
            receiver,
            generation: 0,
            transport,
            completed: false,
        };

        let error = stream.next().await.expect_err("trap");

        assert!(!error.is_duplicate_entry());
    }

    #[tokio::test]
    async fn fatal_event_is_classified_as_connection_failure() {
        let transport = RouterOsTransport::new(RouterOsConnectionConfig::plaintext_for_test());
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        sender
            .send(Event::Fatal {
                reason: "actor stopped".to_string(),
            })
            .expect("fatal event");
        let mut stream = RouterOsCommandStream {
            action: "print".to_string(),
            receiver,
            generation: 0,
            transport,
            completed: false,
        };

        assert_eq!(
            stream.next().await.expect_err("fatal").kind,
            RouterOsErrorKind::Fatal
        );
    }

    #[tokio::test]
    async fn concurrent_failed_cold_start_attempts_connect_once_during_backoff() {
        let mut config = RouterOsConnectionConfig::plaintext_for_test();
        config.address = "127.0.0.1:0".to_string();
        let transport = RouterOsTransport::new(config);
        let attempts = (0..32).map(|_| {
            let transport = transport.clone();
            async move {
                transport
                    .send_command(
                        "cold start",
                        CommandBuilder::new()
                            .command("/system/identity/print")
                            .build(),
                    )
                    .await
            }
        });

        let results = futures::future::join_all(attempts).await;
        let snapshot = transport.snapshot().await;

        assert!(results.iter().all(|result| result.is_err()));
        assert_eq!(snapshot.connect_attempt_total, 1);
        assert_eq!(snapshot.backoff_total, 1);
        assert!(snapshot.degraded);
        assert!(snapshot.retry_after.is_some());
    }

    #[tokio::test]
    async fn stale_generation_failure_does_not_change_retry_state() {
        let transport = RouterOsTransport::new(RouterOsConnectionConfig::plaintext_for_test());
        {
            let mut state = transport.state.lock().await;
            state.generation = 2;
        }

        transport.record_failure(1).await;

        let snapshot = transport.snapshot().await;
        assert_eq!(snapshot.backoff_total, 0);
        assert!(!snapshot.degraded);
        assert!(snapshot.retry_after.is_none());
    }

    #[tokio::test]
    async fn late_success_from_invalidated_generation_preserves_backoff() {
        let transport = RouterOsTransport::new(RouterOsConnectionConfig::plaintext_for_test());
        {
            let mut state = transport.state.lock().await;
            state.generation = 7;
            state.consecutive_failures = 2;
            state.retry_at = Some(Instant::now() + Duration::from_secs(10));
            state.device = None;
        }

        transport.record_success(7).await;

        let snapshot = transport.snapshot().await;
        assert!(snapshot.degraded);
        assert!(snapshot.retry_after.is_some());
    }

    #[tokio::test]
    async fn operation_level_bypass_attempts_reconnect_during_backoff() {
        let mut config = RouterOsConnectionConfig::plaintext_for_test();
        config.address = "127.0.0.1:0".to_string();
        let transport = RouterOsTransport::new(config);
        {
            let mut state = transport.state.lock().await;
            state.consecutive_failures = 1;
            state.retry_at = Some(Instant::now() + Duration::from_secs(30));
        }
        let command = || {
            CommandBuilder::new()
                .command("/system/identity/print")
                .build()
        };

        let blocked = match transport.send_command("normal command", command()).await {
            Ok(_) => panic!("normal command must respect reconnect backoff"),
            Err(error) => error,
        };
        assert_eq!(blocked.kind, RouterOsErrorKind::Backoff);
        assert_eq!(transport.snapshot().await.connect_attempt_total, 0);

        let bypassed = match transport
            .send_command_bypassing_backoff("cleanup command", command())
            .await
        {
            Ok(_) => panic!("test endpoint cannot connect"),
            Err(error) => error,
        };
        assert_ne!(bypassed.kind, RouterOsErrorKind::Backoff);
        assert_eq!(transport.snapshot().await.connect_attempt_total, 1);
    }
}
