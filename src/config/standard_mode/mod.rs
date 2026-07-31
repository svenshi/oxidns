// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Canonical Standard Mode intent, validation, and configuration compilation.
//!
//! Standard Mode is a control-plane compiler. It never participates in the DNS
//! request path and does not perform filesystem or runtime lifecycle work.

mod compiler;
mod migration;
mod model;
mod template;
mod validation;

pub use compiler::{StandardCapabilities, compile_standard_intent, standard_intent_revision};
pub use migration::{StandardIntentDecodeError, decode_standard_intent};
pub use model::{
    CURRENT_STANDARD_SCHEMA, StandardCapabilityExplanation, StandardCompilationExplanation,
    StandardDiagnostic, StandardDiagnosticSeverity, StandardGeneratedConfig,
    StandardGenerationSummary, StandardIntent, StandardIntentMapping, StandardMigration,
    StandardPathBoundary, StandardPlan, StandardPriorityRow, StandardTagMap,
};
pub use template::{
    StandardTemplateExpansion, StandardTemplateKind, StandardTemplateParameters,
    expand_standard_template,
};
pub use validation::{normalize_standard_intent, validate_standard_intent};

#[cfg(test)]
mod tests;
