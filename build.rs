// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

fn main() {
    println!("cargo:rerun-if-env-changed=TARGET");

    if let Ok(target) = std::env::var("TARGET") {
        println!("cargo:rustc-env=OXIDNS_BUILD_TARGET={target}");
    }
}
