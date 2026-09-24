# Testing Strategy

This document defines how to choose validation for OxiDNS changes and how local
checks relate to CI. Tests should be proportional to risk, but feature gates,
DNS semantics, lifecycle behavior, and cross-platform code require broader
coverage than the edited file alone suggests.

`justfile` is the source of truth for local gate implementations. Workspace
`package.json` scripts define WebUI and docs commands, while
`.github/workflows/rust-ci.yml`, `.github/workflows/webui-ci.yml`, and
`.github/workflows/docs-ci.yml` define CI coverage. This document intentionally
describes selection criteria rather than copying their command bodies.

## Validation Ladder

Use the fastest useful level while iterating, then widen before handoff.

### 1. Compile the edited surface

```bash
cargo check
```

For feature-gated work, compile both sides of the gate and the affected bundles.
Use the current feature and bundle names from `Cargo.toml` and the relevant
recipes from `justfile`.

### 2. Run focused tests

```bash
cargo test <filter>
cargo test --test plugin_integration <filter>
cargo test --test message_hickory_compat
```

Keep unit tests close to policy-heavy code. Use integration tests for plugin
registration, dependency resolution, quick setup, server startup, protocol
wiring, and live request flows.

### 3. Run lint without tests

```bash
just lint
```

The recipe body in `justfile` defines the current formatting, toolchain, target,
feature, and warning policy.

### 4. Run the normal repository gate

```bash
just check
```

This is the expected pre-PR gate. Inspect `justfile` rather than duplicating its
current constituent commands here.

### 5. Run the feature matrix when required

```bash
just check-matrix
```

Use it for Cargo feature changes, optional dependency changes, protocol gating,
and broad module moves across cfg boundaries. Its prerequisites and exact
coverage are defined by `justfile`.

The slower pairwise feature powerset is available as:

```bash
just check-powerset
```

Whether and when CI runs this recipe is defined by
`.github/workflows/rust-ci.yml`.

## Change-to-Test Mapping

| Area | Focused validation | Broader gate |
|---|---|---|
| Package-only Rust refactor | Affected module tests and `cargo check` | `just check`; matrix if cfg boundaries moved |
| Plugin config or factory | Plugin unit tests and matching `plugin_integration` filters | `cargo test --test plugin_integration`, `just check` |
| Sequence/dependency graph | Sequence and registry tests, relevant integration filters | Full plugin integration suite |
| Cache semantics | `cargo test cache` plus persistence/API filters as affected | Plugin integration and all-feature tests |
| Message/wire/rcode/TTL behavior | Proto/core tests and Hickory compatibility | Workspace all-feature tests; hot-path review when relevant |
| UDP/TCP server | Server unit tests and live ephemeral-port integration | All-feature tests on CI platforms |
| TLS/DoH/DoQ/HTTP3 | Protocol-specific tests with feature enabled | Feature matrix and cross-platform CI |
| API route or payload | Handler/router tests | All-feature tests and affected WebUI checks |
| RouterOS/ipset/nftset | Pure manager/parser tests with mocks | Linux CI with required system packages; integration tests |
| Service/upgrade/path handling | CLI parsing and path/archive unit tests | Platform CI or explicit platform limitation report |
| WebUI | Focused frontend test and type-check scripts from `webui/package.json` | Additional package scripts and `.github/workflows/webui-ci.yml` jobs according to scope |
| Docs site | Link/content review | Build/check scripts from `docs/package.json` and `.github/workflows/docs-ci.yml` |
| Cargo dependency/features | Targeted compile with feature off/on | `just check-matrix` and CI |

## Feature and Platform Coverage

Every public bundle consumed by CI or release packaging is a contract. Its name,
membership, and default status come from `Cargo.toml`; its expected runtime
surface comes from `src/build_info.rs`, feature-gating tests, and packaged
configuration. Do not assume an all-feature build proves narrower bundles.

The supported toolchains, platforms, bundles, documentation checks, dependency
checks, and feature combinations are the active job matrices in
`.github/workflows/rust-ci.yml`. Do not infer them from an inventory in this
guide.

Local checks cannot replace cfg-specific CI for an unavailable target. Keep
platform logic small, put shared policy in portable modules, and report every
affected target from the source `cfg` and workflow matrix that was not locally
verified.

## Network-Test Rules

- Bind ephemeral ports instead of fixed ports.
- Use bounded timeouts on accepts, reads, writes, shutdown, and background task
  completion.
- Prefer local mock upstreams over public DNS services.
- Avoid sleeps as synchronization; use readiness signals or bounded polling.
- Make cleanup deterministic so a failed assertion does not leave listeners or
  global runtime state behind.
- Serialize tests only when a real global resource requires it, and document
  the reason.
- Test malformed, timeout, closed-channel, and partial-startup paths in
  addition to successful traffic.

Some integration tests bind local sockets and may require running outside a
restricted sandbox. That environment requirement must not be mistaken for a
product failure.

## DNS Correctness Rules

Changes that touch responses must cover relevant invariants:

- Request ID and question preservation.
- QTYPE/QCLASS-sensitive response classification.
- Positive TTL selection and zero-TTL behavior.
- NXDOMAIN/NODATA negative TTL and SOA minimum handling.
- CNAME completeness for the requested type.
- Truncation and transport behavior.
- EDNS, DNSSEC DO/CD bits, and ECS keying where applicable.
- RCODE and control-flow propagation through nested sequences.

Compatibility with Hickory is an additional signal, not permission to replace
OxiDNS-specific semantics.

## Test Placement

- Unit tests belong beside logic-heavy modules.
- Large package tests may use a `tests/` child module split by policy area.
- `tests/plugin_integration.rs` owns cross-plugin and live server behavior.
- `tests/feature_gating.rs` owns positive and negative compilation-surface
  expectations visible at runtime.
- `tests/message_hickory_compat.rs` owns codec compatibility behavior.

## Failure Triage

When a broad gate fails:

1. Re-run the smallest failing test or compilation target with full output.
2. Classify it as logic, feature gating, platform, timing, shared global state,
   or environment setup.
3. Fix the underlying ownership or synchronization problem; do not increase
   timeouts without evidence that the bound is legitimately too small.
4. Re-run the focused failure, then the gate that originally exposed it.
5. Report commands actually run and any remaining untested environment.

Never call a test flaky without repeated evidence and a recorded failure mode.
