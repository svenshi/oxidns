// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Immutable EDNS option rules compiled once per filter plugin.
//! Empty rules and requests with no rejected options preserve shared EDNS
//! storage.

use super::config::{EdnsFilterConfig, EdnsOptionsPolicy};
use crate::proto::{EdnsCode, Message};

#[derive(Debug, Default)]
pub(super) struct EdnsOptionsFilter {
    policy: EdnsOptionsPolicy,
    allowlist: Vec<u16>,
    denylist: Vec<u16>,
}

impl From<EdnsFilterConfig> for EdnsOptionsFilter {
    fn from(config: EdnsFilterConfig) -> Self {
        let mut allowlist = config.allowlist;
        let mut denylist = config.denylist;
        allowlist.sort_unstable();
        allowlist.dedup();
        denylist.sort_unstable();
        denylist.dedup();
        Self {
            policy: config.policy,
            allowlist,
            denylist,
        }
    }
}

impl EdnsOptionsFilter {
    fn allows(&self, code: u16) -> bool {
        let base_allowed = match self.policy {
            EdnsOptionsPolicy::Preserve => true,
            EdnsOptionsPolicy::Strip => false,
            EdnsOptionsPolicy::StripUnknown => EdnsCode::from(code).is_iana_registered(),
        };
        base_allowed
            && (self.allowlist.is_empty() || self.allowlist.binary_search(&code).is_ok())
            && self.denylist.binary_search(&code).is_err()
    }

    pub(super) fn apply(&self, request: &mut Message) {
        if self.policy == EdnsOptionsPolicy::Preserve
            && self.allowlist.is_empty()
            && self.denylist.is_empty()
        {
            return;
        }
        if let Some(edns) = request.edns_mut().as_mut()
            && edns
                .options()
                .iter()
                .any(|option| !self.allows(option.code()))
        {
            edns.retain_options(|option| self.allows(option.code()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Edns, EdnsFlags, EdnsLocal, EdnsOption};

    fn filtered(filter: &EdnsOptionsFilter, mut request: Message) -> Message {
        filter.apply(&mut request);
        request
    }

    fn request(codes: &[u16]) -> Message {
        let mut message = Message::new();
        *message.edns_mut() = Some(Edns::new_with_param(
            4096,
            2,
            1,
            EdnsFlags {
                dnssec_ok: true,
                z: 9,
            },
            codes
                .iter()
                .map(|&code| EdnsOption::Local(EdnsLocal::new(code, vec![1, 2, 3])))
                .collect(),
        ));
        message
    }

    #[test]
    fn filters_are_deletion_only_and_preserve_original_request() {
        let original = request(&[65001, 20, 8, 10, 15, 0, 4, 27, 20292, 26946, 65535]);
        for (policy, allowlist, denylist, expected) in [
            (
                EdnsOptionsPolicy::Preserve,
                vec![],
                vec![],
                vec![65001, 20, 8, 10, 15, 0, 4, 27, 20292, 26946, 65535],
            ),
            (EdnsOptionsPolicy::Strip, vec![8, 65001], vec![], vec![]),
            (
                EdnsOptionsPolicy::StripUnknown,
                vec![],
                vec![],
                vec![20, 8, 10, 15, 20292, 26946],
            ),
            (
                EdnsOptionsPolicy::StripUnknown,
                vec![20, 65001],
                vec![],
                vec![20],
            ),
            (
                EdnsOptionsPolicy::Preserve,
                vec![8, 10, 15, 8],
                vec![10, 10],
                vec![8, 15],
            ),
            (
                EdnsOptionsPolicy::Preserve,
                vec![],
                vec![65001],
                vec![20, 8, 10, 15, 0, 4, 27, 20292, 26946, 65535],
            ),
        ] {
            let filter = EdnsOptionsFilter::from(EdnsFilterConfig {
                policy,
                allowlist,
                denylist,
                ..Default::default()
            });
            let filtered = filtered(&filter, original.clone());
            let edns = filtered.edns().as_ref().unwrap();
            assert_eq!(
                edns.options()
                    .iter()
                    .map(EdnsOption::code)
                    .collect::<Vec<_>>(),
                expected
            );
            let original_edns = original.edns().as_ref().unwrap();
            assert_eq!(edns.udp_payload_size(), original_edns.udp_payload_size());
            assert_eq!(edns.raw_ttl(), original_edns.raw_ttl());
            for option in edns.options() {
                assert!(original_edns.options().contains(option));
            }
            assert_eq!(original_edns.options().len(), 11);
        }
    }

    #[test]
    fn no_op_filters_keep_edns_storage_shared() {
        let original = request(&[20, 65001]);
        for config in [
            EdnsFilterConfig::default(),
            EdnsFilterConfig {
                denylist: vec![65002],
                ..Default::default()
            },
        ] {
            let filtered = filtered(&EdnsOptionsFilter::from(config), original.clone());
            assert_eq!(filtered, original);
            assert_eq!(
                filtered.edns().as_ref().unwrap().options().as_ptr(),
                original.edns().as_ref().unwrap().options().as_ptr()
            );
        }
        let filter = EdnsOptionsFilter::from(EdnsFilterConfig {
            policy: EdnsOptionsPolicy::Strip,
            ..Default::default()
        });
        assert!(filtered(&filter, Message::new()).edns().is_none());
        assert_eq!(filtered(&filter, request(&[])), request(&[]));
    }

    #[test]
    fn compiled_lists_are_sorted_and_deduplicated() {
        let filter = EdnsOptionsFilter::from(EdnsFilterConfig {
            allowlist: vec![15, 8, 15, 10],
            denylist: vec![65001, 10, 65001],
            ..Default::default()
        });
        assert_eq!(filter.allowlist, [8, 10, 15]);
        assert_eq!(filter.denylist, [10, 65001]);
    }

    #[test]
    fn opaque_registered_option_survives_wire_roundtrip() {
        let mut original = request(&[20, 65001]);
        original.edns_mut().as_mut().unwrap().set_ext_rcode(0);
        let decoded = Message::from_bytes(&original.to_bytes().unwrap()).unwrap();
        let filter = EdnsOptionsFilter::from(EdnsFilterConfig {
            policy: EdnsOptionsPolicy::StripUnknown,
            ..Default::default()
        });
        let filtered = filtered(&filter, decoded);
        let decoded = Message::from_bytes(&filtered.to_bytes().unwrap()).unwrap();
        assert_eq!(
            decoded.edns().as_ref().unwrap().options(),
            &original.edns().as_ref().unwrap().options()[..1]
        );
        assert_eq!(
            decoded.edns().as_ref().unwrap().raw_ttl(),
            original.edns().as_ref().unwrap().raw_ttl()
        );
    }
}
