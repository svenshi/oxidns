// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `learn_domain` executor plugin.
//!
//! This executor observes the current DNS flow and appends request qnames to a
//! `dynamic_domain_set` provider. It is a side-effect stage: DNS request and
//! response content are never modified.

mod config;
mod executor;

#[cfg(test)]
mod tests;

pub use executor::LearnDomainFactory;
