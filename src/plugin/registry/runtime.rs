// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Global plugin runtime lifecycle and application-control integration.

use std::panic::AssertUnwindSafe;
#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use futures::FutureExt;
use futures::future::BoxFuture;
use tokio::sync::Mutex as AsyncMutex;

use super::{
    PluginRegistry, PluginRuntime, lock_mutex, read_rwlock, try_global_catalog, write_rwlock,
};
use crate::config::types::Config;
use crate::infra::control::{AppController, ControlRequestError};
use crate::infra::error::{DnsError, Result};
use crate::infra::network::outbound;

#[cfg(debug_assertions)]
#[derive(Debug)]
pub(super) struct TestRuntimeGuard {
    pub(super) _guard: tokio::sync::OwnedMutexGuard<()>,
}

#[cfg(debug_assertions)]
fn test_runtime_lock() -> Arc<AsyncMutex<()>> {
    static TEST_RUNTIME_LOCK: OnceLock<Arc<AsyncMutex<()>>> = OnceLock::new();
    TEST_RUNTIME_LOCK
        .get_or_init(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

#[cfg(debug_assertions)]
static SERIALIZE_RUNTIME_FOR_TESTS: AtomicBool = AtomicBool::new(false);

#[cfg(debug_assertions)]
pub fn enable_runtime_test_serialization() {
    SERIALIZE_RUNTIME_FOR_TESTS.store(true, Ordering::Relaxed);
}

#[derive(Debug)]
pub struct PluginRuntimeManager {
    pub(super) current: RwLock<Option<Arc<PluginRuntime>>>,
    controller: Mutex<Option<Arc<AppController>>>,
    lifecycle: Arc<AsyncMutex<()>>,
}

/// Candidate resources owned exclusively by one serialized startup operation.
/// This session never escapes the managed task, so cancelling a caller's wait
/// cannot strand a candidate between initialization and activation.
pub(crate) struct StartupSession {
    manager: Arc<PluginRuntimeManager>,
    candidate: Option<Arc<PluginRuntime>>,
    #[cfg(debug_assertions)]
    test_guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl StartupSession {
    /// Build plugins without publishing an active runtime or sending readiness.
    /// Application-specific routes and listeners can still fail afterwards.
    pub(crate) async fn initialize(&mut self, config: Config) -> Result<Arc<PluginRuntime>> {
        if self.candidate.is_some() {
            return Err(DnsError::runtime("startup session already initialized"));
        }
        let mut candidate = PluginRegistry::new();
        candidate.load_catalog(try_global_catalog()?);
        #[cfg(debug_assertions)]
        candidate.set_test_runtime_guard(self.test_guard.take());
        let candidate = Arc::new(candidate);
        let previous = write_rwlock(&self.manager.current).take();
        if let Some(previous) = previous {
            previous.destroy().await;
        }
        // Own the candidate before any fallible operation so the common error
        // path can destroy partial initialization and clear outbound state.
        self.candidate = Some(candidate.clone());
        outbound::clear_global();
        outbound::install_global(&config.network.outbound)?;
        candidate
            .clone()
            .init_plugins_with_runtime_controls(
                config.plugins,
                cfg!(feature = "api") && config.api.http.is_some(),
            )
            .await?;
        Ok(candidate)
    }

    fn activate(&mut self) -> Result<()> {
        let candidate = self
            .candidate
            .take()
            .ok_or_else(|| DnsError::runtime("startup completed without initializing plugins"))?;
        // Publication and readiness are synchronous and remain under the same
        // lifecycle permit as initialization and destruction. No later startup
        // step can fail, and another lifecycle operation cannot retire the
        // runtime while readiness hooks are creating maintenance tasks.
        write_rwlock(&self.manager.current).replace(candidate.clone());
        candidate.notify_ready();
        Ok(())
    }

    async fn abort(&mut self) {
        if let Some(candidate) = self.candidate.take() {
            candidate.destroy().await;
            outbound::clear_global();
        }
    }
}

impl PluginRuntimeManager {
    pub(super) fn new() -> Self {
        Self {
            current: RwLock::new(None),
            controller: Mutex::new(None),
            lifecycle: Arc::new(AsyncMutex::new(())),
        }
    }

    pub fn current_runtime(&self) -> Option<Arc<PluginRuntime>> {
        read_rwlock(&self.current).clone()
    }

    /// Run complete standalone startup, including activation, as one operation.
    pub async fn init_runtime(self: &Arc<Self>, config: Config) -> Result<Arc<PluginRuntime>> {
        self.start_with(move |session| Box::pin(async move { session.initialize(config).await }))
            .await
    }

    /// The application supplies its own startup work here, including API setup
    /// and failure cleanup. The owned task always finishes activation or abort
    /// even if its caller drops the returned future after the operation starts.
    pub(crate) async fn start_with<T, F>(self: &Arc<Self>, start: F) -> Result<T>
    where
        T: Send + 'static,
        F: for<'a> FnOnce(&'a mut StartupSession) -> BoxFuture<'a, Result<T>> + Send + 'static,
    {
        // Tests must acquire their serialization permit before lifecycle: the
        // current runtime may need lifecycle access to release that test
        // permit.
        #[cfg(debug_assertions)]
        let test_guard = if SERIALIZE_RUNTIME_FOR_TESTS.load(Ordering::Relaxed) {
            Some(test_runtime_lock().lock_owned().await)
        } else {
            None
        };
        let lifecycle_guard = self.lifecycle.clone().lock_owned().await;
        let manager = self.clone();
        tokio::spawn(async move {
            let _lifecycle_guard = lifecycle_guard;
            let mut session = StartupSession {
                manager,
                candidate: None,
                #[cfg(debug_assertions)]
                test_guard,
            };
            // A plugin/setup panic is an initialization failure too. Catch it
            // while the session still owns its candidate so ordinary cleanup
            // can release already-initialized predecessors.
            let outcome = AssertUnwindSafe(start(&mut session))
                .catch_unwind()
                .await
                .unwrap_or_else(|_| Err(DnsError::runtime("runtime startup panicked")));
            match outcome {
                Ok(output) => match session.activate() {
                    Ok(()) => Ok(output),
                    Err(error) => {
                        session.abort().await;
                        Err(error)
                    }
                },
                Err(error) => {
                    session.abort().await;
                    Err(error)
                }
            }
        })
        .await
        .map_err(|error| DnsError::runtime(format!("runtime startup task failed: {error}")))?
    }

    pub async fn destroy_runtime(self: &Arc<Self>) {
        self.stop_with(|| Box::pin(async {}), || Box::pin(async {}))
            .await;
    }

    /// Stop application listeners and plugins under the startup lifecycle lock.
    /// Like startup, teardown retains ownership until cleanup actually
    /// finishes.
    pub(crate) async fn stop_with<F, G>(self: &Arc<Self>, before_plugins: F, after_plugins: G)
    where
        F: FnOnce() -> BoxFuture<'static, ()> + Send + 'static,
        G: FnOnce() -> BoxFuture<'static, ()> + Send + 'static,
    {
        let lifecycle_guard = self.lifecycle.clone().lock_owned().await;
        let manager = self.clone();
        let task = tokio::spawn(async move {
            let _lifecycle_guard = lifecycle_guard;
            before_plugins().await;
            let previous = write_rwlock(&manager.current).take();
            if let Some(previous) = previous {
                previous.destroy().await;
            }
            outbound::clear_global();
            after_plugins().await;
        });
        if let Err(error) = task.await {
            tracing::error!(%error, "runtime destruction task failed");
        }
    }

    /// The manager is the single authoritative owner of the application
    /// controller. Runtimes never carry their own copy, so swapping a runtime
    /// can neither lose nor race the controller.
    pub fn set_controller(&self, controller: Arc<AppController>) {
        *lock_mutex(&self.controller) = Some(controller);
    }

    pub fn clear_controller(&self) {
        *lock_mutex(&self.controller) = None;
    }

    fn controller(&self) -> Option<Arc<AppController>> {
        lock_mutex(&self.controller).clone()
    }

    pub fn request_app_reload(&self) -> Result<()> {
        let controller = self.controller().ok_or_else(|| {
            DnsError::plugin("reload executor requires application control context")
        })?;
        controller.request_reload().map_err(|err| match err {
            ControlRequestError::ReloadBusy => {
                DnsError::plugin("reload is already pending or in progress")
            }
            ControlRequestError::CommandChannelClosed => {
                DnsError::plugin("application reload command channel is closed")
            }
        })
    }

    pub fn request_app_restart(&self) -> Result<()> {
        let controller = self
            .controller()
            .ok_or_else(|| DnsError::plugin("restart requires application control context"))?;
        controller
            .request_restart()
            .map_err(|err| DnsError::plugin(err.to_string()))
    }

    pub async fn reload_provider(&self, tag: &str) -> Result<()> {
        let runtime = self
            .current_runtime()
            .ok_or_else(|| DnsError::plugin("provider reload requires an initialized runtime"))?;
        runtime.reload_provider(tag).await
    }

    pub fn plugin_count(&self) -> usize {
        self.current_runtime()
            .map_or(0, |runtime| runtime.plugin_count())
    }

    pub fn server_plugin_count(&self) -> usize {
        self.current_runtime()
            .map_or(0, |runtime| runtime.server_plugin_count())
    }

    #[cfg(test)]
    async fn set_current_runtime_for_test(&self, runtime: Arc<PluginRuntime>) {
        let _guard = self.lifecycle.lock().await;
        let previous = write_rwlock(&self.current).take();
        if let Some(previous) = previous {
            previous.destroy().await;
        }
        write_rwlock(&self.current).replace(runtime);
    }
}

static GLOBAL_MANAGER: OnceLock<Arc<PluginRuntimeManager>> = OnceLock::new();

pub fn global_manager() -> Arc<PluginRuntimeManager> {
    GLOBAL_MANAGER
        .get_or_init(|| Arc::new(PluginRuntimeManager::new()))
        .clone()
}

pub fn current_runtime() -> Option<Arc<PluginRuntime>> {
    global_manager().current_runtime()
}

/// Start and activate a standalone runtime using the same managed lifecycle as
/// application startup. Cancellation of the observer cannot omit activation.
pub async fn init(config: Config) -> Result<Arc<PluginRuntime>> {
    global_manager().init_runtime(config).await
}

pub async fn destroy_runtime() {
    global_manager().destroy_runtime().await;
}

pub fn set_app_controller(controller: Arc<AppController>) {
    global_manager().set_controller(controller);
}

pub fn clear_app_controller() {
    global_manager().clear_controller();
}

pub fn request_app_reload() -> Result<()> {
    global_manager().request_app_reload()
}

pub fn request_app_restart() -> Result<()> {
    global_manager().request_app_restart()
}

pub async fn reload_provider(tag: &str) -> Result<()> {
    global_manager().reload_provider(tag).await
}

pub fn plugin_count() -> usize {
    global_manager().plugin_count()
}

pub fn server_plugin_count() -> usize {
    global_manager().server_plugin_count()
}

#[cfg(test)]
pub async fn reset_runtime_for_test() {
    destroy_runtime().await;
    clear_app_controller();
}

#[cfg(test)]
pub async fn set_current_runtime_for_test(runtime: Arc<PluginRuntime>) {
    global_manager().set_current_runtime_for_test(runtime).await;
}
