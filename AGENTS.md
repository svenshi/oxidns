# Repository Guidelines

## Project Focus

- OxiDNS is a high-performance, plugin-driven DNS server written in Rust.
- The current project already includes UDP/TCP/DoT/DoQ/DoH server and upstream support, sequence-based policy orchestration, TTL-aware cache with negative caching, fallback chains, local and synthetic answers, query/response rewriting, ECS handling, dual-stack selection, provider-backed domain/IP rule sets, management APIs, health endpoints, metrics, and system integrations such as `ipset`, `nftset`, and MikroTik route sync.
- Prefer designs that preserve the core request path: `server -> DnsContext -> matcher/executor/provider pipeline -> upstream or side effects -> response`.

## Project Structure & Module Organization

- `src/main.rs` parses top-level CLI options, dispatches foreground startup or service mode, and keeps binary-only entry concerns thin.
- `src/lib.rs` exposes the library surface used by tests and embedding scenarios, including `api`, `app`, `cli`, `config`, `core`, `infra`, `message`, and `plugin`.
- `src/cli/` contains command definitions, parsing, command dispatch, CLI output, and option-to-runtime adapter code.
- `src/app/` contains foreground startup orchestration for wiring config, runtime, API, plugins, and graceful shutdown/reload flows.
- `src/api/` contains the management/control and health HTTP endpoints plus API route macros under `src/api/macros.rs`.
- `src/message/` contains OxiDNS's DNS message model and wire codec implementation.
- `src/core/` is the DNS execution core and should stay focused on `DnsContext`, request lifecycle state, and reusable rule matching primitives.
- `src/infra/` contains project infrastructure shared by CLI, API, app, and plugins: errors, clocks, environment helpers, service management, task orchestration, TTL cache primitives, observability/logging/metrics, build info, upgrade support, and networking.
- `src/config/` defines the YAML schema and validation for runtime configuration.
- `src/infra/network/` contains listeners, protocol transports, TLS setup, upstream resolution, bootstrap logic, pooling, and networking helpers.
- `src/plugin/` is the main extension surface and is split into server, executor, matcher, and provider categories.
- `src/plugin/server/` handles inbound DNS protocols, including UDP, TCP, QUIC, and HTTP-based DNS with dedicated HTTP/2 and HTTP/3 support under `src/plugin/server/http/`.
- `src/plugin/executor/` contains request processors such as `sequence`, `forward`, `cache`, `fallback`, `hosts`, `arbitrary`, `redirect`, `ecs_handler`, `ttl`, `dual_selector`, observability plugins, and system-integration plugins.
- `src/plugin/matcher/` contains rule matchers for qname/qtype/qclass, client IP, response IP, CNAME, response presence, RCODE, marks, env, random rollout, rate limits, and related predicates.
- `src/plugin/provider/` contains reusable domain/IP datasets consumed by matchers and executors.
- Service-management implementation lives in `src/infra/service.rs`; `src/cli/service.rs` only adapts CLI service options.
- `crates/macros/` provides proc-macros used by the plugin registration system (`register_plugin_factory!` and related derives).
- `crates/ripset/` is a pure-Rust Linux netlink implementation for ipset/nftset operations, used by the ipset and nftset executor plugins.
- `crates/proto/` contains the low-level DNS wire protocol types (header, name, question, record, rdata) that back `src/message/`.
- `crates/zoneparser/` is a standalone zone-file parser used for loading hosts and local zone data.
- `tests/plugin_integration.rs` covers config parsing, plugin registry wiring, sequence quick-setup, and live server integration.
- `tests/message_hickory_compat.rs` validates message codec compatibility behavior against Hickory.
- `config.yaml` is the canonical runnable default configuration for the current plugin composition.
- `README.md` and `README_EN.md` describe the architecture and capability set; keep them aligned with behavior changes.
- WebUI-specific guidance lives in `ai/webui.md`; follow it for changes under `webui/`.

## Build, Test, and Development Commands

**Toolchain note:** `rustfmt.toml` uses `unstable_features = true`, so formatting and the pre-commit hook both require the nightly toolchain (`cargo +nightly fmt`). Install it with `rustup toolchain install nightly` if needed.

**Git hooks:** Run `just install-hooks` once per clone to activate the pre-commit hook (`cargo +nightly fmt --check` + `cargo +nightly clippy -- -D warnings`).

**Preferred quality gates (via `just`):**
- `just check` — full gate: fmt check + clippy (`-D warnings`) + tests. Run this before opening a PR.
- `just fix` — auto-applies fmt and Clippy fixes; use during active development.
- `just lint` — fmt check + clippy only, no tests; faster iteration cycle.

**Individual commands:**
- `cargo check` — fastest sanity check during iteration.
- `cargo build --release` — builds the optimized binary.
- `cargo run -- -c config.yaml` — runs OxiDNS with the default config.
- `cargo run --release -- -c config.yaml` — preferred for performance-sensitive validation.
- `cargo run -- -c config.yaml -l debug` — overrides the log level for local debugging.
- `cargo test` — runs all unit and integration tests.
- `cargo test --test plugin_integration` — runs the end-to-end plugin/config integration suite.
- `cargo test <filter>` — runs tests whose names match the filter string (e.g., `cargo test cache` runs all cache-related tests).
- `cargo test --test plugin_integration <filter>` — runs a specific integration test by name.
- `cargo +nightly fmt` — formats code; nightly is required due to unstable rustfmt features.
- `cargo +nightly clippy --all-targets --all-features -- -D warnings` — lints with warnings as errors; required to match CI and the pre-commit hook.

## Coding Style & Naming Conventions

- Rust 2024 edition; format with `cargo +nightly fmt`.
- Use `snake_case` for functions and fields, `CamelCase` for types, and `SCREAMING_SNAKE_CASE` for constants.
- Keep modules cohesive and place helpers close to the feature they serve.
- Comments should be written in English.
- For plugin registration patterns, implementation guidelines, and platform-specific guarding rules, see [PLUGIN_DEV.md](PLUGIN_DEV.md).

## Performance & Architecture Principles

- Treat the request hot path as a first-class design constraint. Avoid unnecessary allocation, cloning, parsing, locking, or blocking I/O in per-request code.
- Prefer work that can be done once at startup or plugin initialization over work repeated for every query.
- Reuse connections and transport state through the existing upstream pool design instead of creating one-off connections on the fast path.
- Respect DNS semantics when touching cache, fallback, rewrite, or synthetic-response code, especially TTL and negative-cache behavior.
- For plugin-specific hot-path rules and composability principles, see [PLUGIN_DEV.md](PLUGIN_DEV.md).

## Testing Guidelines

- Use Rust's built-in test framework and keep focused unit tests close to logic-heavy modules.
- Prefer ephemeral ports, bounded timeouts, and deterministic inputs for network-facing tests.
- Run at least `cargo test` for behavior changes.
- For plugin-specific testing rules (integration test placement, feature gating, trigger conditions), see [PLUGIN_DEV.md](PLUGIN_DEV.md).

## Configuration & Documentation

- If a change adds or renames plugin types, config fields, default behaviors, or supported protocols, update `README.md` and `README_EN.md` in the same change when applicable.
- When preparing a release, follow the standalone workflow in `ai/release-process.md` for tag-based changelog generation, Cargo version bumps, and release-note updates.
- For the full plugin documentation and WebUI sync checklist (`docs/`, `webui/lib/plugin-definitions/`, `config.yaml`), see [PLUGIN_DEV.md](PLUGIN_DEV.md).

## Cargo Feature Conventions

See [PLUGIN_DEV.md](PLUGIN_DEV.md) for the full feature system description, naming rules, the four-step checklist for adding a feature-gated plugin, and the required build verification commands.

## Commit & Pull Request Guidelines

- Use Conventional Commits, for example `feat(cache): add negative cache persistence`.
- Keep commit messages short, action-oriented, and scoped to the subsystem when possible.
- PRs should describe behavior changes, protocol or platform scope, config impact, and the test commands that were run.
- Call out any change that affects the request hot path, default config behavior, or cross-platform support.
