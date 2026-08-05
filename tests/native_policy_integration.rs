// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg(feature = "standard")]

use std::path::PathBuf;

#[test]
fn native_policy_fixture_validates_without_product_mode_types() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native_policy.yaml");
    let summary =
        oxidns::config::validate_file(&path).expect("native YAML fixture should validate");
    assert_eq!(summary.plugin_count, 5);
    assert_eq!(
        summary.dependency_graph.init_order,
        [
            "native_cache_default",
            "native_forward_default",
            "native_path_default",
            "native_udp",
            "native_tcp",
        ]
    );
}
