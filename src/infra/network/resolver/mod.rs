// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared name resolution helpers for outbound network clients.

mod cache;
mod client;
mod endpoint;
mod name;
mod query;

pub(crate) use endpoint::NameserverConfig;
pub(crate) use name::NameResolver;
