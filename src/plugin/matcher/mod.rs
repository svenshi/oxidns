// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later
//! Matcher plugin category.
//!
//! Matchers are pure predicates used by executors such as `sequence` to branch
//! on request or response state without embedding policy logic directly into
//! the server path.
//!
//! Typical matcher inputs include:
//!
//! - query name, type, and class;
//! - client IP or derived request metadata;
//! - response content such as answer IPs, CNAMEs, or rcode; and
//! - internal marks, random rollout state, or environment-derived signals.
//!
//! Matchers should stay fast and side-effect free. They read from
//! [`DnsContext`] and return a boolean decision through [`Matcher::is_match`].

use std::sync::Arc;

use crate::core::context::DnsContext;
use crate::infra::error::{DnsError, Result};
use crate::plugin::Plugin;

mod control;

pub mod any_match;
pub mod client_ip;
pub mod cname;
pub mod env;
pub mod false_matcher;
pub mod has_cname;
pub mod has_resp;
pub mod has_wanted_ans;
pub mod mark;
pub mod ptr_ip;
pub mod qclass;
pub mod qname;
pub mod qtype;
pub mod question;
pub mod random;
pub mod rate_limiter;
pub mod rcode;
pub mod resp_ip;
pub(crate) mod rules;
pub mod string_exp;
pub mod time;
pub mod true_matcher;

#[cfg(any(feature = "api", test))]
pub(crate) use control::{MatcherRuntimeControl, MatcherRuntimeMode};

#[allow(dead_code)]
pub trait Matcher: Plugin {
    /// is_match checks if the DNS request context matches certain criteria
    fn is_match(&self, context: &mut DnsContext) -> bool;
}

#[derive(Debug)]
pub struct MatcherRef {
    /// Concrete matcher instance used by this instruction.
    matcher: Arc<dyn Matcher>,
    /// Whether matcher result should be logically negated (`!matcher`).
    reverse: bool,
    /// Optional runtime override shared by all references to a configured tag.
    #[cfg(any(feature = "api", test))]
    runtime_control: Option<Arc<MatcherRuntimeControl>>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum MatcherEvaluation {
    Matched,
    NotMatched,
    #[cfg(any(feature = "api", test))]
    AlwaysTrue {
        matched: bool,
    },
    #[cfg(any(feature = "api", test))]
    AlwaysFalse {
        matched: bool,
    },
}

impl MatcherEvaluation {
    #[inline]
    pub(crate) fn is_match(self) -> bool {
        match self {
            Self::Matched => true,
            Self::NotMatched => false,
            #[cfg(any(feature = "api", test))]
            Self::AlwaysTrue { matched } => matched,
            #[cfg(any(feature = "api", test))]
            Self::AlwaysFalse { matched } => matched,
        }
    }

    #[cfg(feature = "_sequence-step-recording")]
    #[inline]
    pub(crate) fn outcome(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::NotMatched => "not_matched",
            #[cfg(any(feature = "api", test))]
            Self::AlwaysTrue { matched: true } => "always_true_matched",
            #[cfg(any(feature = "api", test))]
            Self::AlwaysTrue { matched: false } => "always_true_not_matched",
            #[cfg(any(feature = "api", test))]
            Self::AlwaysFalse { matched: true } => "always_false_matched",
            #[cfg(any(feature = "api", test))]
            Self::AlwaysFalse { matched: false } => "always_false_not_matched",
        }
    }
}

impl MatcherRef {
    pub fn new(matcher: Arc<dyn Matcher>, reverse: bool) -> Self {
        Self {
            matcher,
            reverse,
            #[cfg(any(feature = "api", test))]
            runtime_control: None,
        }
    }

    #[cfg(any(feature = "api", test))]
    pub(crate) fn with_runtime_control(
        matcher: Arc<dyn Matcher>,
        reverse: bool,
        runtime_control: Arc<MatcherRuntimeControl>,
    ) -> Self {
        Self {
            matcher,
            reverse,
            runtime_control: Some(runtime_control),
        }
    }

    pub fn tag(&self) -> &str {
        self.matcher.tag()
    }

    pub fn is_match(&self, context: &mut DnsContext) -> bool {
        self.evaluate(context).is_match()
    }

    pub(crate) fn evaluate(&self, context: &mut DnsContext) -> MatcherEvaluation {
        #[cfg(any(feature = "api", test))]
        if let Some(control) = &self.runtime_control {
            match control.mode() {
                MatcherRuntimeMode::AlwaysFalse => {
                    return MatcherEvaluation::AlwaysFalse {
                        matched: self.reverse,
                    };
                }
                MatcherRuntimeMode::AlwaysTrue => {
                    return MatcherEvaluation::AlwaysTrue {
                        matched: !self.reverse,
                    };
                }
                MatcherRuntimeMode::Normal => {}
            }
        }

        let matched = self.matcher.is_match(context);
        if matched != self.reverse {
            MatcherEvaluation::Matched
        } else {
            MatcherEvaluation::NotMatched
        }
    }
}

/// Parse matcher expression and optional reverse prefix (`!`).
///
/// Examples:
/// - `$qname` -> `(false, "$qname")`
/// - `!$qname` -> `(true, "$qname")`
/// - `!qname domain:example.com` -> `(true, "qname domain:example.com")`
pub(super) fn parse_matcher_expr(raw: &str) -> Result<(bool, &str)> {
    let matcher_expr = raw.trim_start();
    if let Some(matcher_expr) = matcher_expr.strip_prefix('!') {
        let matcher_expr = matcher_expr.trim_start();
        if matcher_expr.is_empty() {
            return Err(DnsError::plugin(format!(
                "invalid matcher reference: '{}'",
                raw
            )));
        }
        Ok((true, matcher_expr))
    } else {
        Ok((false, matcher_expr))
    }
}
