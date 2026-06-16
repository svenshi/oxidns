// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later
//! Executor plugin category.
//!
//! Executors are the active stages in a OxiDNS sequence pipeline. They can:
//!
//! - mutate the request before upstream resolution;
//! - call upstream resolvers and populate a response;
//! - rewrite or filter an existing response;
//! - update request-local marks and request-local metadata; and
//! - trigger side effects such as metrics or external system integration.
//!
//! Execution is centered on [`Executor::execute`] and
//! [`Executor::execute_with_next`]. Simple executors act on the current
//! [`DnsContext`] and return [`ExecStep`] to either continue or stop. Advanced
//! executors can wrap downstream stages through the `next` continuation model
//! used by the `sequence` plugin.
//!
//! Hot-path expectations:
//!
//! - avoid unnecessary allocation and blocking work per request;
//! - push expensive initialization into plugin startup when possible; and
//! - keep side effects off the latency-sensitive response path unless required
//!   for correctness.

use async_trait::async_trait;

use crate::core::context::DnsContext;
use crate::core::error::Result;
use crate::plugin::Plugin;
pub use crate::plugin::executor::sequence::chain::ExecutorNext;

#[cfg(feature = "plugin-arbitrary")]
pub mod arbitrary;
pub mod black_hole;
pub mod cache;
#[cfg(feature = "plugin-cron")]
pub mod cron;
pub mod debug_print;
#[cfg(feature = "plugin-download")]
pub mod download;
pub mod drop_resp;
pub mod dual_selector;
pub mod ecs_handler;
pub mod fallback;
pub mod forward;
pub mod forward_edns0opt;
pub mod hosts;
#[cfg(feature = "plugin-http-request")]
pub mod http_request;
#[cfg(feature = "plugin-ip-selector")]
pub mod ip_selector;
#[cfg(feature = "plugin-ipset")]
pub mod ipset;
#[cfg(feature = "plugin-dynamic-domain")]
pub mod learn_domain;
#[cfg(feature = "metrics")]
pub mod metrics_collector;
#[cfg(feature = "plugin-ipset")]
pub mod nftset;
#[cfg(feature = "plugin-query-recorder")]
pub mod query_recorder;
pub mod query_summary;
#[cfg(feature = "api")]
pub(crate) mod rdata_json;
pub mod redirect;
pub mod reload;
pub mod reload_provider;
#[cfg(feature = "plugin-reverse-lookup")]
pub mod reverse_lookup;
#[cfg(feature = "plugin-mikrotik")]
pub mod ros_address_list;
#[cfg(feature = "plugin-script")]
pub mod script;
pub mod sequence;
pub mod sleep;
#[cfg(any(feature = "plugin-http-request", feature = "plugin-script"))]
pub(crate) mod template;
pub mod ttl;
#[cfg(feature = "plugin-upgrade")]
pub mod upgrade;

// Helper macro to continue to next chain node if present
#[macro_export]
macro_rules! continue_next {
    ($next:expr, $ctx:expr) => {{
        if let Some(next) = $next {
            next.next($ctx).await
        } else {
            Ok($crate::plugin::executor::ExecStep::Next)
        }
    }};
}

#[async_trait]
pub trait Executor: Plugin {
    fn with_next(&self) -> bool {
        false
    }

    /// Execute the plugin's logic on a DNS request context.
    ///
    /// Return [`ExecStep`] to instruct the sequence engine how to advance.
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep>;

    /// Execute around the downstream chain represented by `next`.
    async fn execute_with_next(
        &self,
        context: &mut DnsContext,
        next: Option<ExecutorNext>,
    ) -> Result<ExecStep> {
        let result = self.execute(context).await?;
        match result {
            ExecStep::Next => continue_next!(next, context),
            ExecStep::Stop | ExecStep::Return => Ok(result),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ExecStep {
    /// Continue executing the current chain or report natural completion.
    Next,
    /// Stop the current chain immediately.
    Stop,
    /// Return control to the caller without resuming the current sequence.
    Return,
}
