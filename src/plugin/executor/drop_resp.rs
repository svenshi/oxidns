// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `drop_resp` executor plugin.
//!
//! Clears the current response from [`DnsContext`].
//!
//! This plugin is useful when a previous executor produced a response but a
//! later policy requires re-querying or rebuilding output. It only resets
//! `context.response`/final packet output and keeps request metadata/marks
//! untouched.

use async_trait::async_trait;
use serde::Deserialize;

use crate::config::types::PluginConfig;
use crate::core::context::{DnsContext, ExecutionPathEvent};
use crate::infra::error::{DnsError, Result};
use crate::plugin::executor::{ExecStep, Executor};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::plugin_factory;

#[derive(Debug)]
struct DropResp {
    tag: String,
    reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DropRespConfig {
    /// Stable machine-readable reason included in opt-in execution recording.
    reason: Option<String>,
}

#[async_trait]
impl Plugin for DropResp {
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
impl Executor for DropResp {
    #[hotpath::measure]
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        context.clear_response();
        if context.execution_path_enabled() {
            context.push_execution_path_event(ExecutionPathEvent::new(
                self.tag.as_str(),
                None,
                "decision",
                Some(self.tag.as_str()),
                self.reason.as_deref().unwrap_or("response_dropped"),
            ));
        }
        Ok(ExecStep::Next)
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("drop_resp")]
pub struct DropRespFactory;

impl PluginFactory for DropRespFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        let config = plugin_config
            .args
            .clone()
            .map(serde_yaml_ng::from_value::<DropRespConfig>)
            .transpose()
            .map_err(|err| DnsError::plugin(format!("failed to parse drop_resp config: {err}")))?
            .unwrap_or_default();
        let reason = config
            .reason
            .map(|reason| reason.trim().to_string())
            .filter(|reason| !reason.is_empty());
        if reason.as_deref().is_some_and(|reason| {
            !reason
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        }) {
            return Err(DnsError::plugin(
                "drop_resp reason must contain only lowercase ASCII letters, digits, or '_'",
            ));
        }
        Ok(UninitializedPlugin::Executor(Box::new(DropResp {
            tag: plugin_config.tag.clone(),
            reason,
        })))
    }

    fn quick_setup(&self, tag: &str, _param: Option<String>) -> Result<UninitializedPlugin> {
        Ok(UninitializedPlugin::Executor(Box::new(DropResp {
            tag: tag.to_string(),
            reason: None,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::executor::ExecStep;
    use crate::plugin::test_utils::test_context;

    #[tokio::test]
    async fn test_execute_clears_response() {
        let plugin = DropResp {
            tag: "drop_resp".to_string(),
            reason: None,
        };
        let mut ctx = test_context();
        ctx.set_response(crate::proto::Message::new());

        let step = plugin
            .execute(&mut ctx)
            .await
            .expect("execute should succeed");
        assert!(matches!(step, ExecStep::Next));
        assert!(ctx.response().is_none());
    }

    #[tokio::test]
    async fn test_execute_records_configured_reason_when_enabled() {
        let plugin = DropResp {
            tag: "drop_resp".to_string(),
            reason: Some("domestic_ip_mismatch".to_string()),
        };
        let mut ctx = test_context();
        ctx.enable_execution_path();
        ctx.set_response(crate::proto::Message::new());

        plugin.execute(&mut ctx).await.unwrap();

        let event = ctx.execution_path_events().last().unwrap();
        assert_eq!(event.kind, "decision");
        assert_eq!(event.tag.as_deref(), Some("drop_resp"));
        assert_eq!(event.outcome, "domestic_ip_mismatch");
    }
}
