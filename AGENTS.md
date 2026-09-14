# Repository Guidelines

## Project Focus

- OxiDNS is a high-performance, plugin-driven DNS server written in Rust.
- Prefer designs that preserve the core request path: `server -> DnsContext -> matcher/executor/provider pipeline -> upstream or side effects -> response`.
- Derive the current capability set from `Cargo.toml` features, `src/plugin/*/mod.rs`, `src/build_info.rs`, and the runnable configuration files. Do not treat capability inventories in prose as authoritative.

## Sources of Truth

- Prefer executable project state over AI prose when facts can change: `Cargo.toml` for features and dependencies, `justfile` for local quality gates, `.github/workflows/` for CI and release behavior, `src/plugin/*/mod.rs` and registration code for compiled plugins, `src/config/` plus `config*.yaml` for configuration, and workspace `package.json` files for frontend/docs commands.
- `AGENTS.md` and `ai/*.md` define stable intent, architecture constraints, decision criteria, and workflow rationale. They should point to project files instead of duplicating evolving inventories, versions, command bodies, or target matrices.
- If prose disagrees with code, configuration, tests, or workflows, use the executable source for the current fact and update the stale guidance as part of the same change when it is in scope.

## Project Structure & Module Organization

- `src/main.rs` parses top-level CLI options, dispatches foreground startup or service mode, and keeps binary-only entry concerns thin.
- `src/lib.rs` declares the library surface used by the binary, tests, and embedding scenarios; its exports are authoritative.
- `src/build_info.rs` reports compiled bundles, enabled features, and runtime plugin capabilities. It lives at the crate root because it depends on both infrastructure constants and the plugin catalog; do not move plugin-aware capability reporting into `infra`.
- `src/cli/` contains command definitions, parsing, command dispatch, CLI output, and option-to-runtime adapter code.
- `src/app/` contains foreground startup orchestration for wiring config, runtime, API, plugins, and graceful shutdown/reload flows.
- `src/api/` contains the management/control and health HTTP endpoints plus API route macros under `src/api/macros.rs`.
- `src/core/` is the DNS execution core and should stay focused on `DnsContext`, request lifecycle state, and reusable rule matching primitives.
- `src/infra/` contains subsystem-neutral infrastructure shared by CLI, API, app, and plugins: errors, clocks, environment helpers, line-oriented I/O, service management, task orchestration, TTL cache primitives, observability/logging/metrics, upgrade support, and networking.
- Keep the dependency direction one-way: `plugin` may use `infra`, but `infra` must not depend on plugin traits, registries, or plugin-specific models. Shared code belongs in `infra` only when its API and semantics are useful outside the plugin system.
- `src/config/` defines the YAML schema and validation for runtime configuration.
- `src/infra/network/` contains listeners, protocol transports, TLS setup, upstream resolution, bootstrap logic, pooling, and networking helpers.
- `src/infra/io/` contains reusable file and stream helpers, including line-oriented rule loading shared by providers.
- `src/infra/upgrade/` separates release discovery, download, archive handling, progress reporting, and binary/WebUI installation while exposing upgrade orchestration through `mod.rs`.
- `src/plugin/` is the main extension surface and is split into server, executor, matcher, and provider categories. The category `mod.rs` files and factory registration are the authoritative plugin inventory.
- Category-local lifecycle, parsing, metrics, and protocol/provider semantics stay within their owning plugin package unless the abstraction is genuinely subsystem-neutral.
- Service-management implementation lives in `src/infra/service.rs`; `src/cli/service.rs` only adapts CLI service options.
- Workspace members and their dependency relationships are declared by the root and member `Cargo.toml` files. Each member owns its local API and stability policy.
- `tests/plugin_integration.rs` covers config parsing, plugin registry wiring, sequence quick-setup, and live server integration.
- `tests/message_hickory_compat.rs` validates message codec compatibility behavior against Hickory.
- `config*.yaml` files are the canonical runnable configuration profiles for their corresponding bundles.
- `README.md` and `README_EN.md` describe the architecture and capability set; keep them aligned with behavior changes.
- Detailed internal architecture and dependency-boundary guidance lives in `ai/architecture.md`.
- WebUI-specific guidance lives in `ai/webui.md`; follow it for changes under `webui/`.

## Build, Test, and Development Commands

`justfile`, `rustfmt.toml`, and the active CI workflows are the source of truth for toolchains and quality-gate command bodies. Inspect the relevant recipe before running or documenting a gate.

**Git hooks:** Run `just install-hooks` once per clone. The installed files under
`.githooks/` define the actual pre-commit checks.

**Preferred quality gates (via `just`):**
- `just check` — normal pre-PR gate.
- `just fix` — repository-managed formatting and lint fixes during development.
- `just lint` — faster lint-only iteration.
- Use the feature/bundle recipes declared in `justfile` when `Cargo.toml` feature wiring changes.

Use `cargo check` for a fast compile sanity check and focused `cargo test`
filters while iterating. For exact runtime CLI syntax, inspect `src/cli/` or
`oxidns --help`; use `config*.yaml` as runnable examples. Copy formatting,
linting, bundle, and full-gate invocations from `justfile` or `.githooks/`
rather than maintaining command variants here.

## Coding Style & Naming Conventions

- Follow the Rust edition declared in the root `Cargo.toml` and format through
  the repository recipe in `justfile`.
- Use `snake_case` for functions and fields, `CamelCase` for types, and `SCREAMING_SNAKE_CASE` for constants.
- Keep modules cohesive and place helpers close to the feature they serve.
- Comments should be written in English.
- For plugin registration patterns, implementation guidelines, and platform-specific guarding rules, see [ai/plugin-dev.md](ai/plugin-dev.md).

## Performance & Architecture Principles

- Treat the request hot path as a first-class design constraint. Avoid unnecessary allocation, cloning, parsing, locking, or blocking I/O in per-request code.
- Prefer work that can be done once at startup or plugin initialization over work repeated for every query.
- Reuse connections and transport state through the existing upstream pool design instead of creating one-off connections on the fast path.
- Respect DNS semantics when touching cache, fallback, rewrite, or synthetic-response code, especially TTL and negative-cache behavior.
- Performance-sensitive changes must follow the hot-path and resource-safety review rules in `ai/performance.md`.
- For plugin-specific hot-path rules and composability principles, see [ai/plugin-dev.md](ai/plugin-dev.md).

## Testing Guidelines

- Use Rust's built-in test framework and keep focused unit tests close to logic-heavy modules.
- Prefer ephemeral ports, bounded timeouts, and deterministic inputs for network-facing tests.
- Select validation proportionally from the recipes in `justfile`, workspace `package.json` scripts, and affected CI workflow. Run focused checks while iterating and the applicable repository gate before handoff.
- Use `ai/testing-strategy.md` only for selection criteria and correctness invariants, not as a command inventory.
- For plugin-specific testing rules (integration test placement, feature gating, trigger conditions), see [ai/plugin-dev.md](ai/plugin-dev.md).

## Configuration & Documentation

- Update `README.md` and `README_EN.md` only when their user-facing capability or
  prominent-default summaries are affected; detailed config fields belong in
  the plugin reference and schema-driven surfaces.
- Use the changed contract and its maintained representations—Rust schema, WebUI definitions, examples, API docs, and translations—to determine synchronization. `ai/change-impact-matrix.md` provides trigger criteria only.
- When preparing a release, treat `.github/workflows/release.yml`, related workflows, Cargo manifests, and packaging scripts as the executable release contract; use `ai/release-process.md` for sequencing and review criteria.
- For plugin changes, inspect the actual Rust registration/config, `webui/lib/plugin-definitions/`, locale trees, documentation trees, and `config*.yaml`; update only representations triggered by the change.

## Cargo Feature Conventions

`Cargo.toml` is the authoritative feature graph and bundle definition. Category module `cfg` guards, `src/build_info.rs`, `tests/feature_gating.rs`, `justfile`, and `.github/workflows/rust-ci.yml` are the authoritative integration and validation surfaces. `ai/plugin-dev.md` explains the stable design rules behind them.

## Operations & Maintenance

- Follow `ai/operations-runbook.md` for deployment preflight, health/readiness, diagnosis, reload, upgrade, and rollback procedures.
- Follow `ai/maintenance.md` for dependency updates, toolchain changes, feature hygiene, workspace crate maintenance, and recurring documentation audits.
- Security reports and vulnerability handling follow `SECURITY.md`; do not put secrets or private DNS data into public logs or issues.

## Commit & Pull Request Guidelines

- Use Conventional Commits, for example `feat(cache): add negative cache persistence`.
- Keep commit messages short, action-oriented, and scoped to the subsystem when possible.
- PRs should describe behavior changes, protocol or platform scope, config impact, and the test commands that were run.
- Call out any change that affects the request hot path, default config behavior, or cross-platform support.
