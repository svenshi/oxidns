// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `learn_domain` executor plugin.
//!
//! This executor observes the current DNS flow and appends request qnames (or
//! their registrable domains) to the `dynamic_domain_set` named by `provider`.
//! `rule_kind` selects exact-name, request-subtree, or registrable-domain
//! learning. Registrable-domain lookup uses an embedded ICANN + PRIVATE public
//! suffix list without request-time I/O. The provider owns persistence and
//! background tasks; this executor only enqueues
//! rules or waits for a flush when `async: false`. DNS messages are never
//! modified.

mod config;
mod domain;
mod executor;

#[cfg(test)]
mod tests;

pub use executor::LearnDomainFactory;
