# Standard / Expert Mode Backend Decoupling

Status: implemented and accepted

Baseline commit: `90a5dec`

Implementation branch: `feat/webui_standard_mode`

## Product boundary

Standard and Expert are WebUI workspaces. The OxiDNS process accepts native YAML, reports compiled capabilities, validates candidate configuration, applies it transactionally, and emits raw runtime facts. It does not own workspace intent, templates, generated mappings, product diagnosis, or mode ownership.

The target flow is:

```text
WebUI intent -> TypeScript compiler -> native YAML -> validate -> apply -> runtime
raw query events -> WebUI explainer
```

The Cargo `standard` bundle remains a build-time capability bundle and is not a product-mode API.

## Frozen artifacts

The following product-planning documents are frozen byte-for-byte against `90a5dec` and are excluded from this migration:

- `docs/docs/standard-mode-plan.md`
- `docs/i18n/en/docusaurus-plugin-content-docs/current/standard-mode-plan.md`

Every phase gate verifies that both paths have no diff from the baseline commit.

## Delivery phases

### 0. Migration baseline

- Capture representative Standard intent and normalized YAML semantic fixtures before deleting the Rust compiler.
- Cover default configuration, every upstream strategy, cache and ECS isolation, filtering and local policies, smart routing, dedicated listeners, dynamic learning, advanced rules, schema 1-6 migrations, and missing capabilities.
- Compare normalized intent, diagnostic codes, YAML semantic trees, stable labels, priority rows, path boundaries, mappings, and managed files. Serializer-only formatting and hashes derived solely from formatting are ignored.

### 1. Mode-neutral configuration transactions

- Add adjacent, crash-safe `.config-transaction.json`, `.config-transaction.last.json`, and `.config-history.json` stores.
- Add candidate validation in the real configuration directory, optimistic version checks, bounded request bodies, status, healthy-history listing, and restore previews.
- Make all YAML writes share one generic lock and atomic replacement helper.
- Retain the healthy running YAML and version in `AppController` so rollback restores the runtime truth even after a save-only operation changed disk state.
- Keep history best-effort after a successful runtime commit; treat journal completion as critical.

### 2. Browser-owned compiler

- Make TypeScript the sole Standard intent compiler and revision generator.
- Preserve native plugin composition, stable tags, priorities, path-scoped cache/ECS behavior, upstream strategies, managed path names, and top-level preservation semantics.
- Use build capabilities for diagnostics and generic backend validation as the final apply gate.

### 3. Browser-owned workspace and explanation

- Move import/export, preview, Expert Copy, Expert Analysis, and templates to the browser.
- Store active intent and generated mappings in opaque WebUI JSON with CAS; store instance-scoped templates in IndexedDB.
- Interpret raw query records only in the WebUI and degrade to raw facts when revisions differ.

### 4. Remove mode-specific backend control plane

- Delete `/api/standard/*`, `src/api/standard_mode.rs`, and `src/config/standard_mode/`.
- Remove mode-specific product types, errors, journal formats, asset stores, and backend diagnosis.
- Do not delete orphaned dynamic-learning files automatically.

### 5. Acceptance and handoff

- Run Rust, feature-bundle, cross-platform, WebUI, documentation, local runtime, and isolated-machine gates.
- Verify all removed endpoints return 404, the backend-source coupling scan is empty, frozen documents are unchanged, and generated YAML validates and rolls back correctly.
- Record limitations, code removal, generic capabilities, test evidence, and operational migration notes in a separate handoff document.

## Compatibility decisions

- No deprecated Standard backend API is retained because the branch is unreleased.
- Old `.standard-*` sidecars are deliberately not migrated.
- Existing opaque WebUI JSON remains readable and is normalized by the new client.
- Restore is preview-only and never mutates DNS state.
- UI state persistence failure never rolls back a successful DNS apply.
- Generated comments and query context are opaque native configuration data to the backend.
