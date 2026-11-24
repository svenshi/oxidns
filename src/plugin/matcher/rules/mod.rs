// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rule parsing, source resolution, and provider binding for matcher plugins.

mod config;
mod numeric;
mod providers;
mod sources;

pub(crate) use config::{
    parse_enum_rules_from_value, parse_quick_setup_rules, parse_rule_list, parse_rules_from_value,
    validate_non_empty_rules,
};
pub(crate) use numeric::parse_u16_rules;
pub(crate) use providers::{
    ensure_domain_capable_providers, ensure_ip_capable_providers, provider_dependency_specs,
    resolve_provider_tags,
};
#[cfg(test)]
pub(crate) use sources::parse_ip_prefix_matcher;
pub(crate) use sources::{
    parse_domain_rules_and_set_tags, parse_ip_rules_and_set_tags, provider_tags_from_rules,
    split_rule_sources, validate_non_empty_domain_rules_or_set_tags,
    validate_non_empty_ip_rules_or_set_tags,
};
