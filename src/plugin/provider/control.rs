// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Serialized runtime reload control for provider instances.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use thiserror::Error;
use tokio::sync::Mutex;

use crate::infra::error::DnsError;
use crate::plugin::provider::Provider;
#[cfg(feature = "api")]
use crate::plugin::provider::ProviderRuntimeStatus;

#[derive(Debug, Error)]
pub(crate) enum ProviderReloadError {
    #[error("provider '{tag}' reload is already in progress")]
    Busy { tag: String },
    #[error(transparent)]
    Failed(#[from] DnsError),
}

impl ProviderReloadError {
    pub(crate) fn into_dns_error(self) -> DnsError {
        match self {
            Self::Busy { .. } => DnsError::plugin(self.to_string()),
            Self::Failed(error) => error,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ProviderRuntimeControl {
    provider: Arc<dyn Provider>,
    reload_lock: Arc<Mutex<()>>,
    accepting_reloads: AtomicBool,
}

impl ProviderRuntimeControl {
    pub(crate) fn new(provider: Arc<dyn Provider>) -> Self {
        Self {
            provider,
            reload_lock: Arc::new(Mutex::new(())),
            accepting_reloads: AtomicBool::new(true),
        }
    }

    pub(crate) async fn reload(&self) -> Result<(), ProviderReloadError> {
        self.ensure_accepting_reloads()?;
        let guard =
            self.reload_lock
                .clone()
                .try_lock_owned()
                .map_err(|_| ProviderReloadError::Busy {
                    tag: self.provider.tag().to_string(),
                })?;
        self.ensure_accepting_reloads()?;
        let provider = self.provider.clone();
        let tag = provider.tag().to_string();
        let result = tokio::spawn(async move {
            let _guard = guard;
            provider.reload().await
        })
        .await
        .map_err(|error| {
            ProviderReloadError::Failed(DnsError::runtime(format!(
                "provider '{tag}' reload task failed: {error}"
            )))
        })?;
        result.map_err(Into::into)
    }

    /// Stop accepting new reloads and wait for the current detached reload.
    ///
    /// Runtime teardown calls this before destroying the provider so an old
    /// snapshot build cannot overlap initialization of its replacement.
    pub(crate) async fn drain(&self) {
        self.accepting_reloads.store(false, Ordering::Release);
        let _guard = self.reload_lock.clone().lock_owned().await;
    }

    fn ensure_accepting_reloads(&self) -> Result<(), ProviderReloadError> {
        if self.accepting_reloads.load(Ordering::Acquire) {
            return Ok(());
        }
        Err(ProviderReloadError::Failed(DnsError::plugin(format!(
            "provider '{}' is shutting down",
            self.provider.tag()
        ))))
    }

    #[cfg(feature = "api")]
    pub(crate) fn status(&self) -> ProviderRuntimeStatus {
        self.provider.runtime_status()
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use tokio::sync::Notify;

    use super::*;
    use crate::infra::error::Result as DnsResult;
    use crate::plugin::Plugin;

    #[derive(Debug)]
    struct BlockingProvider {
        reloads: AtomicUsize,
        started: Notify,
        release: Notify,
    }

    #[async_trait]
    impl Plugin for BlockingProvider {
        fn tag(&self) -> &str {
            "blocking"
        }
    }

    #[async_trait]
    impl Provider for BlockingProvider {
        fn as_any(&self) -> &dyn Any {
            self
        }

        async fn reload(&self) -> DnsResult<()> {
            let reload = self.reloads.fetch_add(1, Ordering::Relaxed);
            if reload == 0 {
                self.started.notify_one();
                self.release.notified().await;
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn concurrent_reload_is_rejected_instead_of_queued() {
        let provider = Arc::new(BlockingProvider {
            reloads: AtomicUsize::new(0),
            started: Notify::new(),
            release: Notify::new(),
        });
        let control = Arc::new(ProviderRuntimeControl::new(provider.clone()));
        let first_control = control.clone();
        let first = tokio::spawn(async move { first_control.reload().await });

        provider.started.notified().await;
        let second = control.reload().await;
        assert!(matches!(
            second,
            Err(ProviderReloadError::Busy { ref tag }) if tag == "blocking"
        ));
        assert_eq!(provider.reloads.load(Ordering::Relaxed), 1);

        provider.release.notify_one();
        first
            .await
            .expect("first reload task should finish")
            .unwrap();
        control.reload().await.unwrap();
        assert_eq!(provider.reloads.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn cancelled_caller_does_not_release_reload_ownership() {
        let provider = Arc::new(BlockingProvider {
            reloads: AtomicUsize::new(0),
            started: Notify::new(),
            release: Notify::new(),
        });
        let control = Arc::new(ProviderRuntimeControl::new(provider.clone()));
        let first_control = control.clone();
        let first = tokio::spawn(async move { first_control.reload().await });

        provider.started.notified().await;
        first.abort();
        assert!(
            first
                .await
                .expect_err("caller task should be cancelled")
                .is_cancelled()
        );

        assert!(matches!(
            control.reload().await,
            Err(ProviderReloadError::Busy { ref tag }) if tag == "blocking"
        ));
        assert_eq!(provider.reloads.load(Ordering::Relaxed), 1);

        provider.release.notify_one();
        let mut completed = false;
        for _ in 0..128 {
            match control.reload().await {
                Ok(()) => {
                    completed = true;
                    break;
                }
                Err(ProviderReloadError::Busy { .. }) => tokio::task::yield_now().await,
                Err(error) => panic!("unexpected reload error: {error}"),
            }
        }
        assert!(
            completed,
            "detached reload should eventually release ownership"
        );
        assert_eq!(provider.reloads.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn drain_waits_for_detached_reload_and_rejects_new_work() {
        let provider = Arc::new(BlockingProvider {
            reloads: AtomicUsize::new(0),
            started: Notify::new(),
            release: Notify::new(),
        });
        let control = Arc::new(ProviderRuntimeControl::new(provider.clone()));
        let first_control = control.clone();
        let first = tokio::spawn(async move { first_control.reload().await });

        provider.started.notified().await;
        first.abort();
        assert!(
            first
                .await
                .expect_err("caller should be cancelled")
                .is_cancelled()
        );

        let drain_control = control.clone();
        let drain = tokio::spawn(async move { drain_control.drain().await });
        tokio::task::yield_now().await;
        assert!(!drain.is_finished());

        provider.release.notify_one();
        drain.await.expect("drain should finish");
        let error = control.reload().await.unwrap_err();
        assert!(error.to_string().contains("shutting down"));
        assert_eq!(provider.reloads.load(Ordering::Relaxed), 1);
    }
}
