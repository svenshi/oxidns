// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Plugin dependency resolver interfaces and initialization context.

use std::sync::Arc;

use crate::infra::error::Result;
use crate::plugin::executor::Executor;
use crate::plugin::executor::forward::DefaultResolverTransports;
use crate::plugin::matcher::MatcherRef;
use crate::plugin::provider::Provider;
use crate::plugin::{
    PluginCreateContext, PluginDependent, PluginHolder, PluginInfo, PluginRegistry,
};

pub trait PluginResolver {
    fn executor(
        &self,
        source_tag: &str,
        field: &str,
        target_tag: &str,
    ) -> Result<Arc<dyn Executor>>;
    fn executor_of_type(
        &self,
        source_tag: &str,
        field: &str,
        target_tag: &str,
        expected_plugin_type: &str,
    ) -> Result<Arc<dyn Executor>>;
    fn matcher_ref(
        &self,
        source_tag: &str,
        field: &str,
        target_tag: &str,
        reverse: bool,
    ) -> Result<MatcherRef>;
    fn provider(
        &self,
        source_tag: &str,
        field: &str,
        target_tag: &str,
    ) -> Result<Arc<dyn Provider>>;
    fn provider_of_type(
        &self,
        source_tag: &str,
        field: &str,
        target_tag: &str,
        expected_plugin_type: &str,
    ) -> Result<Arc<dyn Provider>>;
}

#[derive(Debug)]
pub struct PluginInitContext<'a> {
    registry: Arc<PluginRegistry>,
    tag: String,
    create_context: &'a PluginCreateContext,
}

impl<'a> PluginInitContext<'a> {
    pub(crate) fn new(
        registry: Arc<PluginRegistry>,
        tag: impl Into<String>,
        create_context: &'a PluginCreateContext,
    ) -> Self {
        Self {
            registry,
            tag: tag.into(),
            create_context,
        }
    }

    pub(crate) fn default_resolver_transports(&self) -> Arc<DefaultResolverTransports> {
        let mut slot = super::lock_mutex(&self.registry.default_resolver_transports);
        if let Some(cache) = slot.upgrade() {
            return cache;
        }
        let cache = Arc::new(DefaultResolverTransports::new());
        *slot = Arc::downgrade(&cache);
        cache
    }

    pub fn tag(&self) -> &str {
        &self.tag
    }

    pub fn dependents(&self) -> &[PluginDependent] {
        &self.create_context.dependents
    }

    pub fn plugin(&self, field: &str, target_tag: &str) -> Result<Arc<PluginInfo>> {
        self.registry
            .get_required_plugin(&self.tag, field, target_tag)
    }

    pub fn executor(&self, field: &str, target_tag: &str) -> Result<Arc<dyn Executor>> {
        self.registry
            .get_executor_dependency(&self.tag, field, target_tag)
    }

    pub fn executor_of_type(
        &self,
        field: &str,
        target_tag: &str,
        expected_plugin_type: &str,
    ) -> Result<Arc<dyn Executor>> {
        self.registry.get_executor_dependency_of_type(
            &self.tag,
            field,
            target_tag,
            expected_plugin_type,
        )
    }

    /// Resolve a configured matcher expression with its runtime control.
    ///
    /// `reverse` represents the expression-level `!` modifier. Runtime modes
    /// determine the matcher base result first, then this modifier is applied,
    /// so consumers must keep evaluation inside the returned [`MatcherRef`].
    pub fn matcher_ref(&self, field: &str, target_tag: &str, reverse: bool) -> Result<MatcherRef> {
        self.registry
            .get_matcher_ref_dependency(&self.tag, field, target_tag, reverse)
    }

    pub fn provider(&self, field: &str, target_tag: &str) -> Result<Arc<dyn Provider>> {
        self.registry
            .get_provider_dependency(&self.tag, field, target_tag)
    }

    pub fn provider_of_type(
        &self,
        field: &str,
        target_tag: &str,
        expected_plugin_type: &str,
    ) -> Result<Arc<dyn Provider>> {
        self.registry.get_provider_dependency_of_type(
            &self.tag,
            field,
            target_tag,
            expected_plugin_type,
        )
    }

    pub async fn init_quick_setup(
        &self,
        plugin_type: &str,
        tag: &str,
        param: Option<String>,
    ) -> Result<PluginHolder> {
        let uninitialized = self.registry.clone().quick_setup(plugin_type, tag, param)?;
        let context = PluginCreateContext::default();
        let init_context = PluginInitContext::new(self.registry.clone(), tag, &context);
        uninitialized.init_and_wrap(&init_context).await
    }
}

impl PluginResolver for PluginRegistry {
    fn executor(
        &self,
        source_tag: &str,
        field: &str,
        target_tag: &str,
    ) -> Result<Arc<dyn Executor>> {
        self.get_executor_dependency(source_tag, field, target_tag)
    }

    fn executor_of_type(
        &self,
        source_tag: &str,
        field: &str,
        target_tag: &str,
        expected_plugin_type: &str,
    ) -> Result<Arc<dyn Executor>> {
        self.get_executor_dependency_of_type(source_tag, field, target_tag, expected_plugin_type)
    }

    fn matcher_ref(
        &self,
        source_tag: &str,
        field: &str,
        target_tag: &str,
        reverse: bool,
    ) -> Result<MatcherRef> {
        self.get_matcher_ref_dependency(source_tag, field, target_tag, reverse)
    }

    fn provider(
        &self,
        source_tag: &str,
        field: &str,
        target_tag: &str,
    ) -> Result<Arc<dyn Provider>> {
        self.get_provider_dependency(source_tag, field, target_tag)
    }

    fn provider_of_type(
        &self,
        source_tag: &str,
        field: &str,
        target_tag: &str,
        expected_plugin_type: &str,
    ) -> Result<Arc<dyn Provider>> {
        self.get_provider_dependency_of_type(source_tag, field, target_tag, expected_plugin_type)
    }
}
