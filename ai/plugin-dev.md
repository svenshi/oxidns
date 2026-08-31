# Plugin Development Guide

This document defines stable design criteria for plugin work. Use it when a
change affects plugin architecture, registration, lifecycle, feature gating, or
cross-surface contracts. Derive the current plugin inventory and wiring from the
Rust modules, factory registration, `Cargo.toml`, and tests rather than from this
document.

---

## Plugin Categories

OxiDNS plugins fall into four categories, each with its own trait and directory:

| Category | Trait | Directory | Role |
|----------|-------|-----------|------|
| **Executor** | `Executor` | `src/plugin/executor/` | Process or mutate a request/response in a sequence pipeline |
| **Matcher** | `Matcher` | `src/plugin/matcher/` | Evaluate a boolean predicate on the current `DnsContext` |
| **Provider** | `Provider` | `src/plugin/provider/` | Expose a reusable dataset (domain set, IP set, etc.) |
| **Server** | `Server` | `src/plugin/server/` | Accept inbound DNS traffic over a protocol |

---

## Registration

Register new plugin types with the `#[plugin_factory("type")]` attribute on a unit or empty-braced struct:

```rust
#[derive(Debug, Clone)]
#[plugin_factory("my_plugin")]
pub struct MyPluginFactory;

impl PluginFactory for MyPluginFactory {
    fn create(&self, plugin_config: &PluginConfig, ...) -> Result<UninitializedPlugin> { ... }
}
```

Fall back to `register_plugin_factory!("type", expr)` only when:
- the factory requires state at construction time (e.g. `DualSelectorFactory::new(RecordType::A)`), or
- a single factory struct must register under multiple type names.

---

## Implementation Guidelines

- Include a module-level doc comment that covers: purpose, config shape, dependency expectations, lifecycle, and any hot-path or side-effect behavior.
- Inspect and reuse the abstractions exposed by `src/core/`, `src/plugin/`, and `src/infra/network/` before introducing parallel frameworks.
- Keep platform-specific code clearly guarded — especially Linux-only netlink, `ipset`, and `nftset` paths.
- Management HTTP API integration must compile cleanly without the `api` feature. Gate the module with `#[cfg(feature = "api")]` or keep feature-specific implementations behind internal `cfg` branches when a no-op facade is required.
- Resolve configured matcher dependencies with `PluginInitContext::matcher_ref(field, tag, reverse)` and evaluate the returned `MatcherRef`. Do not extract an `Arc<dyn Matcher>` and apply `!` outside the reference: runtime `always_true` / `always_false` modes fix the matcher base result before each reference applies its own negation.

### Package boundaries

- Keep `mod.rs` focused on the package facade, plugin lifecycle, factory wiring, and high-level orchestration. Split stable responsibilities such as config parsing, models, metrics, persistence, protocol adapters, and management API integration into named sibling modules when the package grows.
- Keep category-specific shared code inside its plugin category. Matcher rule parsing and provider binding belong under `plugin/matcher`; provider formats and selectors belong under `plugin/provider`; server request and connection behavior belong under `plugin/server`.
- Move code into `infra` only when its API is subsystem-neutral and useful outside one plugin category. `infra` must not depend on plugin traits, registries, or plugin-specific models.
- Treat root-crate Rust module paths as internal implementation details. Migrate all in-repository callers during structural changes; add a facade or re-export only for an explicitly documented supported Rust API, not for hypothetical downstream consumers.

### Hot-path rules

- Avoid unnecessary allocation, cloning, parsing, locking, or blocking I/O per request.
- Push expensive initialization into `Plugin::init` rather than repeating it per query.
- Keep side effects (metrics updates, persistence writes, external system calls) off the latency-sensitive response path unless correctness requires otherwise.
- Preserve plugin composability. DNS- or configuration-driven policy normally belongs in a plugin or trait extension; transport framing and connection-lifecycle behavior may remain server-local when that ownership is intrinsic and documented.
- Justify every `Arc`, `DashMap`, queue, or background task added to the core path; watch for lock contention and unbounded state growth.

---

## Cargo Feature Conventions

The `[features]` table in `Cargo.toml` is the only authoritative feature graph.
Use the unguarded and `#[cfg]`-guarded declarations in the plugin category
`mod.rs` files to determine which plugins are always available. Use
`src/build_info.rs` and `tests/feature_gating.rs` to check the user-visible
capability and negative-build behavior.

The graph distinguishes public bundles and granular features from private
aggregators. Follow the current naming families already declared in
`Cargo.toml`; do not copy a feature inventory into prose. Features whose names
start with `_` are implementation details: depend on them through a public
feature and do not document them as operator-facing switches.

### When to add a feature gate

Add a feature gate for any plugin that meets **at least one** of these criteria:

1. Introduces a new optional Cargo dependency edge.
2. Pulls in heavy protocol infrastructure (TLS, HTTP, QUIC).
3. Is not needed for basic DNS forwarding and would be out of scope for a `minimal` build.
4. Has significant runtime side effects (file I/O, background tasks, external system calls) that an operator may want to exclude.

Adding a small predicate or extending an already-compiled plugin does not by
itself require another feature. Preserve the existing bundle contract unless
the change explicitly revises it.

### Integration points for a gated plugin

Update every applicable executable surface in the same change:

1. Declare the public feature and optional dependency edges in `Cargo.toml`,
   and intentionally choose bundle membership there.
2. Gate the category module and every downstream reference with the same public
   feature; provide a safe disabled-feature path where shared code needs one.
3. Update `src/build_info.rs` when the capability is reported to operators.
4. Add positive and negative coverage in `tests/feature_gating.rs` and gate
   affected integration tests in `tests/plugin_integration.rs`.

Select feature-off/on, bundle, and matrix checks from the current recipes in
`justfile`. CI parity and platform coverage are defined by
`.github/workflows/rust-ci.yml`; do not preserve a copied command matrix here.

---

## Testing

- Place unit tests (`#[cfg(test)] mod tests`) inside the plugin's own module, close to the logic under test.
- Add wiring-level tests to `tests/plugin_integration.rs` for: config parsing, dependency resolution, sequence quick-setup, and server integration.
- Gate each integration test behind `#[cfg(feature = "plugin-my-plugin")]` when the plugin is feature-gated.
- Run the focused integration recipe or command that covers registration, config parsing, sequence behavior, or server startup when those paths change; use `justfile` for the current broader gates.
- Cover both success paths and failure paths for any plugin that touches upstream resolution, cache, or cross-plugin dependencies.

---

## Documentation & WebUI Sync

Update only maintained representations of the contract that actually changed:

1. **`docs/`** — sync the relevant Chinese plugin reference page and its English counterpart under `docs/i18n/en/`. Cover behavior, config shape, dependencies, lifecycle, side effects, and examples whenever any of those change.

2. **`webui/lib/plugin-definitions/`** — add or update the entry in the correct category file (`executor.ts`, `matcher.ts`, `provider.ts`, or `server.ts`). The catalog, create dialog, cards, detail drawer, sequence composer, and YAML editor all auto-derive from these definitions.

3. **`README.md` and `README_EN.md`** — update capability summaries only when the user-visible capability or prominent default changes; plugin field details belong in the plugin reference.

4. **`config.yaml`** — update the canonical default config if the change affects the default plugin composition or introduces required new config fields.

Use descriptive plugin tags in examples: `forward_main`, `cache_main`, `udp_server`, `seq_main`, etc. Keep `sequence` examples readable; use tagged reusable plugins once logic becomes non-trivial.
