# Change Impact Matrix

Use this document before implementing and before handing off a change. Its
purpose is to identify synchronized artifacts, compatibility risks, and the
smallest credible validation set. It complements the domain-specific rules in
`AGENTS.md`, `ai/plugin-dev.md`, and `ai/webui.md`.

## General Rule

A change is complete only when every maintained representation of the changed
contract agrees. In OxiDNS, one concept may be represented in Rust code, Cargo
features, YAML examples, management API payloads, WebUI schemas, Chinese and
English documentation, packaging, and release assets.

Do not update unrelated artifacts merely because they exist. Use the trigger
conditions below.

## Impact Matrix

| Change | Required synchronization | Minimum validation |
|---|---|---|
| Internal Rust refactor with unchanged behavior | Module declarations and in-repository imports; architecture notes when boundaries change. Do not add root-crate compatibility facades unless an explicitly supported Rust API requires one | `cargo check`, focused tests, `cargo +nightly fmt --all --check`; use `just check` before merge |
| New, renamed, or removed plugin type | Plugin module/factory, dependency kind, Cargo feature and bundle when gated, `tests/plugin_integration.rs`, plugin docs in both languages, WebUI definition/i18n, README capability lists, example config when relevant | Feature-on/off checks, plugin integration tests, `just check`, WebUI typecheck |
| Plugin config field, enum, default, or validation change | Rust config parser, integration tests, Chinese/English plugin reference, WebUI schema and both locales, `config.yaml` when canonical defaults change, README when behavior is prominent | Focused unit/integration tests, config validation, WebUI typecheck, docs build for non-trivial docs changes |
| Matcher/provider rule syntax change | Parser and error diagnostics, unit tests, `tests/plugin_integration.rs`, Chinese/English configuration docs, affected provider/plugin reference, examples | Focused parser tests plus plugin integration tests |
| Sequence control-flow or quick-setup change | Parser, dependency analysis, runtime chain behavior, graph output, integration tests, configuration docs, WebUI sequence tooling when syntax changes | Sequence unit tests, `tests/plugin_integration.rs`, `just check` |
| Management API route, method, payload, status, auth, or CORS change | Rust handler/router tests, `docs/docs/api.mdx` and English counterpart, WebUI API client/store/types and UI states | Focused API tests, `cargo test`, WebUI test/typecheck/lint as affected |
| WebUI-only behavior or presentation change | Component/store/schema, translation keys and both locales, `ai/webui.md` if conventions change, user docs for workflow changes | `pnpm typecheck`, plus `pnpm lint`, `pnpm test`, and `pnpm build` according to scope |
| Cargo feature or bundle membership change | `Cargo.toml`, cfg guards, `build_info`, feature-gating tests, custom-build assumptions, plugin-dev bundle documentation, user custom-build docs | Target feature disabled/enabled checks, `just check-matrix` when feasible |
| DNS message model, wire codec, or response classification change | `crates/proto`, `src/core`, compatibility tests, and hot-path review when request processing changes | Workspace tests, `tests/message_hickory_compat.rs`, request-path and resource-safety review |
| Server, upstream, resolver, TLS, HTTP, or QUIC behavior change | Feature graph, transport tests, integration tests, protocol docs in both languages, config examples, release target compatibility | Focused transport tests, all-feature tests, slim bundle checks, cross-platform CI where cfg-specific |
| Cache, fallback, rewrite, or synthetic-response semantics change | Unit tests for positive/negative/error paths, plugin integration, metrics/docs when counters change, persistence compatibility when serialized data changes | Focused tests, `cargo test cache` or subsystem filter, plugin integration, hot-path and resource-bound review |
| Persistence or on-disk format change | Reader/writer, version/backward compatibility policy, corruption tests, upgrade/rollback notes, API import/export behavior if exposed | Round-trip, old-fixture, truncated/corrupt input, and recovery tests |
| Service management, upgrade, install, or packaging change | CLI, `src/infra/service.rs` or `src/infra/upgrade/`, Debian/systemd/Docker/scripts, CLI docs in both languages, release workflow if artifact layout changes | Platform-focused tests, config/path tests, archive inspection, affected packaging smoke test |
| Metrics or logging contract change | Metric source, low-cardinality labels, API docs in both languages, WebUI dashboards/labels, operational runbook | Metric rendering tests, affected plugin tests, WebUI typecheck when consumed |
| Dependency or toolchain update | Manifest and lockfile, feature/default-feature review, patched dependency notes, CI actions/runtime versions, affected workspace crates | `just check`, feature matrix for optional deps, platform CI for native/system dependencies |
| Release workflow or artifact naming change | `release.yml`, `custom-build.yml` target mapping, upgrade asset selection, Docker download patterns, release process and user install docs | Workflow review, archive-name tests, dry-run or manual workflow where available |

## Compatibility Questions

The root crate is shipped as a binary application rather than a supported Rust
SDK. Its Rust source paths are not a compatibility axis unless a particular API
is explicitly documented as stable. Evaluate compatibility through the
operator-facing contracts below; do not retain obsolete internal APIs for
hypothetical external crate consumers.

Answer these explicitly for any behavior change:

1. Can an existing YAML configuration start without edits?
2. Does an omitted field preserve its previous default?
3. Can an older persisted file be read by the new binary?
4. Can the current WebUI operate against the new API payload?
5. Does `minimal` still compile without optional protocol and management
   dependencies?
6. Does the change alter DNS TTL, RCODE, truncation, DNSSEC, ECS, or negative
   caching semantics?
7. Does the change alter shutdown, reload, retry, timeout, or side-effect
   ordering?
8. Does a release asset, service path, or WebUI directory need migration?

If any answer is no or uncertain, include a migration or compatibility note in
the PR and the applicable release notes.

## Documentation Sources of Truth

- `AGENTS.md`: concise repository-wide rules and entry points.
- `ai/architecture.md`: internal architecture and dependency boundaries.
- `ai/plugin-dev.md`: plugin registration, features, tests, and docs sync.
- `ai/webui.md`: frontend architecture, i18n, schemas, and visual rules.
- `ai/testing-strategy.md`: validation depth and CI parity.
- `ai/performance.md`: performance evidence and regression rules.
- `ai/operations-runbook.md`: operational diagnosis and recovery.
- `ai/release-process.md`: release preparation and publication verification.
- `docs/docs/` and the English i18n tree: user-facing behavior.

When internal mechanics change without user-visible impact, update AI guidance
only if the rule or workflow changed. When behavior changes, update both
Chinese and English user documentation.

## Pull Request Handoff

Every substantial PR description should state:

- Intent and user/operator impact.
- Config, API, persistence, feature, and platform compatibility.
- Whether the request hot path changed.
- Which synchronized artifacts were updated or why they were not triggered.
- Exact validation commands actually run.
- Remaining unverified platform or environment assumptions.
