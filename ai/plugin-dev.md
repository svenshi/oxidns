# Plugin Development Guide

This document is the single source of truth for writing, registering, testing, documenting, and feature-gating OxiDNS plugins. Read it before adding any new plugin or modifying an existing one.

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
- Reuse existing abstractions (`DnsContext`, `Executor`, `Matcher`, `Provider`, `RequestHandle`, upstream pools, plugin registry) before introducing parallel frameworks.
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
- Preserve plugin composability — new behavior should be added as a plugin or trait extension, not as a server-level special case.
- Justify every `Arc`, `DashMap`, queue, or background task added to the core path; watch for lock contention and unbounded state growth.

---

## Cargo Feature Conventions

OxiDNS uses a three-layer Cargo feature system. Every new plugin must be placed in the correct layer and wired through all four integration points below.

### Three layers

| Layer | Names | Purpose |
|-------|-------|---------|
| **Bundles** | `minimal`, `standard`, `full` (default) | Preset combinations for release artifacts |
| **Granular** | `plugin-*`, `provider-*`, `server-*`, `upstream-*`, `api`, `webui`, `metrics` | One flag per plugin / protocol / surface |
| **Private aggregators** | `_tls-base`, `_http-server`, `_http-client`, `_sequence-step-recording` | Shared optional deps — never enable directly |

**Bundle scope:**
- `minimal` — UDP/TCP listeners and upstreams, `sequence`, `forward`, `cache`, `fallback`, `hosts`, `redirect`, `dual_selector`, `ecs_handler`, `ttl`, `drop_resp`, `black_hole`, `debug_print`, `reload`, all matchers, `domain_set`, `ip_set`. No hyper/rustls/quinn/h2/h3/zoneparser.
- `standard` — `minimal` + management API, WebUI, metrics, DoT/DoH/DoQ, most executor and provider plugins. No MikroTik, no ipset/nftset.
- `full` (default) — `standard` + DoH3, `plugin-mikrotik` (both RouterOS executors), `plugin-ipset`.

### Always-on core plugins (no feature gate)

These plugins are compiled unconditionally as part of `minimal`. Do **not** add a feature gate to them:

`black_hole`, `cache`, `debug_print`, `drop_resp`, `dual_selector`, `ecs_handler`, `fallback`, `forward`, `forward_edns0opt`, `hosts`, `query_summary`, `redirect`, `reload`, `sleep`, `sequence`, `ttl` — and all matchers (`qname`, `qtype`, `qclass`, `client_ip`, `resp_ip`, `cname`, `has_resp`, `rcode`, `mark`, `env`, `random`, `rate_limit`, …) and core providers (`domain_set`, `ip_set`).

### When to add a feature gate

Add a feature gate for any plugin that meets **at least one** of these criteria:

1. Introduces a new optional Cargo dependency (e.g. `rusqlite`, `mikrotik-rs`, `prost`, `zoneparser`).
2. Pulls in heavy protocol infrastructure (TLS, HTTP, QUIC).
3. Is not needed for basic DNS forwarding and would be out of scope for a `minimal` build.
4. Has significant runtime side effects (file I/O, background tasks, external system calls) that an operator may want to exclude.

Adding a new matcher predicate or extending an existing executor's config options does **not** need a feature gate.

### Naming convention

| Category | Pattern | Examples |
|----------|---------|---------|
| Executor or paired executor+provider | `plugin-<noun>` | `plugin-cron`, `plugin-ip-selector`, `plugin-dynamic-domain` |
| Provider only | `provider-<noun>` | `provider-protobuf`, `provider-adguard-rule` |
| Inbound protocol | `server-<proto>` | `server-dot`, `server-doh`, `server-doq`, `server-doh3` |
| Outbound protocol | `upstream-<proto>` | `upstream-dot`, `upstream-doh`, `upstream-doq` |
| Management surface | bare name | `api`, `webui`, `metrics` |

Use kebab-case. Do not use underscores in feature names.

### Checklist for adding a new feature-gated plugin

All four steps must land in the same PR.

**1. `Cargo.toml` — declare the feature and mark deps as optional**

```toml
# `my_plugin` executor (one-line description of purpose).
plugin-my-plugin = ["dep:some-crate"]   # omit dep list if no new Cargo dep
```

Mark any new Cargo dependency `optional = true`:

```toml
some-crate = { version = "1.0", optional = true }
```

Add to the appropriate bundle (`standard` for most plugins; `full` only for Linux-specific or niche integrations):

```toml
standard = [
    ...
    "plugin-my-plugin",
]
```

If the plugin requires the management API:

```toml
plugin-my-plugin = ["api", "dep:some-crate"]
```

**2. `src/plugin/executor/mod.rs` or `src/plugin/provider/mod.rs` — gate the module**

```rust
#[cfg(feature = "plugin-my-plugin")]
pub mod my_plugin;
```

**3. Downstream references — guard any code outside the plugin directory**

```rust
#[cfg(feature = "plugin-my-plugin")]
use crate::plugin::executor::my_plugin::MyFactory;
```

If a shared utility (e.g. `src/config/`) references a symbol only present in a feature-gated module, guard the call with `#[cfg(feature = "...")]` and provide a safe fallback when the feature is absent.

**4. `tests/plugin_integration.rs` — gate integration tests**

```rust
#[cfg(feature = "plugin-my-plugin")]
#[tokio::test]
async fn test_my_plugin_init() -> Result<()> { ... }
```

### Private aggregator features

Never enable `_tls-base`, `_http-server`, `_http-client`, or `_sequence-step-recording` directly. They are implementation details. If your plugin needs outbound HTTP, declare `"_http-client"` as a dependency of your feature in `Cargo.toml`; do not reference `hyper` directly.

### Verifying a new feature

```bash
cargo check --no-default-features --features minimal                       # plugin must be absent
cargo check --no-default-features --features "minimal,plugin-my-plugin"   # must compile cleanly
cargo check --no-default-features --features standard                      # must compile cleanly
cargo check                                                                # full build, must pass
```

---

## Testing

- Place unit tests (`#[cfg(test)] mod tests`) inside the plugin's own module, close to the logic under test.
- Add wiring-level tests to `tests/plugin_integration.rs` for: config parsing, dependency resolution, sequence quick-setup, and server integration.
- Gate each integration test behind `#[cfg(feature = "plugin-my-plugin")]` when the plugin is feature-gated.
- Run `cargo test --test plugin_integration` whenever you change plugin registration, config parsing, sequence behavior, or server startup paths.
- Cover both success paths and failure paths for any plugin that touches upstream resolution, cache, or cross-plugin dependencies.

---

## Documentation & WebUI Sync

When adding or modifying a plugin, update all four artifacts in the same PR:

1. **`docs/`** — sync the relevant Chinese plugin reference page and its English counterpart under `docs/i18n/en/`. Cover behavior, config shape, dependencies, lifecycle, side effects, and examples whenever any of those change.

2. **`webui/lib/plugin-definitions/`** — add or update the entry in the correct category file (`executor.ts`, `matcher.ts`, `provider.ts`, or `server.ts`). The catalog, create dialog, cards, detail drawer, sequence composer, and YAML editor all auto-derive from these definitions.

3. **`README.md` and `README_EN.md`** — update if the change adds or renames plugin types, config fields, default behaviors, or supported protocols.

4. **`config.yaml`** — update the canonical default config if the change affects the default plugin composition or introduces required new config fields.

Use descriptive plugin tags in examples: `forward_main`, `cache_main`, `udp_server`, `seq_main`, etc. Keep `sequence` examples readable; use tagged reusable plugins once logic becomes non-trivial.
