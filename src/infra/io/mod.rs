// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared file and stream handling helpers.

mod fingerprint;
mod text_source;

pub(crate) use fingerprint::FingerprintReader;
// Some facade types are exposed through method signatures and therefore do
// not need to be named by current callers.
#[allow(unused_imports)]
pub(crate) use text_source::{
    LineAnnotations, LineClassifier, SkippedTextFile, TextLine, TextLocation, TextScanError,
    TextSource, TextSourceSession,
};
