---
title: Community, Support, and Contributing
---

OxiDNS welcomes real deployment feedback, documentation improvements, bug fixes, and well-scoped feature proposals. This page explains where to ask questions, how to prepare reports, and which artifacts must stay synchronized in code or documentation changes.

## Choose the Right Channel

| Need | Channel |
| --- | --- |
| Usage questions, configuration, design discussion | [GitHub Discussions](https://github.com/svenshi/oxidns/discussions) or [Telegram @OXIDNS](https://t.me/oxidns) |
| Reproducible software defect | [Bug Report](https://github.com/svenshi/oxidns/issues/new/choose) |
| Feature request with a use case | [Feature Request](https://github.com/svenshi/oxidns/issues/new/choose) |
| Security vulnerability | [Private security report](security.md#report-vulnerabilities-privately) |
| Well-scoped code or documentation improvement | GitHub Pull Request |

Never include passwords, tokens, private keys, private names, client IPs, or complete query history in a public report.

## Write a High-Quality Issue

A bug report should include:

- OxiDNS version, bundle, and commit when applicable.
- Operating system, CPU architecture, and installation method.
- A redacted minimal config and the real `-d/--working-dir`.
- Reproducible query, startup, or reload steps.
- Expected and observed behavior.
- The first causal error and the diagnostics already run.

Following [Operations and Troubleshooting](operations.md) first helps separate product defects from listener, bundle, path, and deployment issues.

## Local Development Setup

The project uses Rust 2024 edition. Common commands:

```bash
cargo check
cargo test
cargo test --test plugin_integration
```

The repository rustfmt configuration requires nightly:

```bash
rustup toolchain install nightly
cargo +nightly fmt --all --check
cargo +nightly clippy --all-targets --all-features -- -D warnings
```

Run the normal pre-PR gate when possible:

```bash
just check
```

Cargo feature, optional dependency, and bundle changes also require the feature matrix. WebUI and platform-specific changes require the corresponding checks in those areas.

## Code Principles

- Preserve the request path: `server -> DnsContext -> matcher/executor/provider -> upstream or side effect`.
- Avoid repeated allocation, parsing, connection setup, locking, or blocking I/O per query.
- Move config parsing, dependency analysis, and reusable state to initialization where possible.
- Cache, fallback, rewrite, and synthetic-response changes must preserve TTL, negative-cache, RCODE, CNAME, and related DNS semantics.
- Network tests use ephemeral ports, bounded timeouts, and deterministic inputs.
- New side effects document platform, privileges, failure policy, resource limits, and cleanup behavior.

## Keep Plugin Artifacts Synchronized

A plugin addition, rename, or behavior change usually updates all of these in one PR:

1. Rust factory, feature, and bundle wiring.
2. `tests/plugin_integration.rs` and relevant unit tests.
3. Chinese and English plugin reference.
4. WebUI plugin definitions and both locales.
5. `README.md` and `README_EN.md` when a prominent capability or protocol changes.
6. `config.yaml` when the default composition or required fields change.

The docs build checks the bilingual file tree, plugin overview versus detailed reference, and plugin reference versus the Rust registry.

## Documentation Contributions

User documentation lives in:

- Chinese: `website/docs/`
- English: `website/i18n/en/docusaurus-plugin-content-docs/current/`

Behavior, configuration, API, and plugin changes should update both languages. Use descriptive tags such as `forward_main`, `cache_main`, and `seq_main`; extract reusable pieces from complex sequences into standalone plugins.

Validate locally:

```bash
cd website
npm ci
npm run check:content
npm run build
```

Broken links and anchors fail the build.

## Pull Request Description

A PR should state:

- Intent and user/operator impact.
- Configuration, API, persistence, feature, and platform compatibility.
- Whether the DNS request hot path changed.
- Synchronized code, WebUI, docs, and configuration artifacts.
- Exact validation commands run.
- Remaining unverified platform or environment assumptions.

Use Conventional Commits, for example `feat(cache): add negative cache persistence`. Keep changes focused and avoid unrelated refactors in the same PR.

## Community Collaboration

Ground discussion in technical evidence, reproducible results, and real use cases. Respect different deployment choices; when disagreeing, explain constraints, risks, and a testable alternative.
