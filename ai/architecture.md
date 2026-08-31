# Internal Architecture Guide

This document is the maintainer-facing architecture contract for OxiDNS. It
explains module ownership, dependency direction, lifecycle boundaries, and the
rules for changing the request path. User-facing architecture belongs in
`docs/docs/architecture-and-design.md`; concise repository instructions remain
in `AGENTS.md`.

## Core Request Path

All DNS transports converge on one processing model:

```text
server -> RequestHandle -> DnsContext -> executor/matcher/provider pipeline
       -> upstream or side effects -> response
```

The responsibilities are deliberately narrow:

- A server plugin accepts one inbound protocol, normalizes transport metadata,
  and hands the request to `RequestHandle`.
- `DnsContext` owns request-local ingress metadata, the request and optional
  response, execution-path events, marks, and typed runtime extensions.
- A matcher evaluates a predicate without owning pipeline control flow.
- An executor performs an action, composes the next executor when supported,
  and returns an `ExecStep`.
- A provider exposes reusable domain, IP, or rule data without becoming a
  second execution pipeline.
- Network upstreams and external side effects stay behind executor or
  infrastructure abstractions; servers must not special-case policy behavior.

Preserve this path when adding protocols or features. A capability that only
one transport can trigger still belongs in the policy layer when its semantics
are DNS- or configuration-driven rather than transport-driven.

## Top-Level Ownership

### Binary and application assembly

- `src/main.rs` is a thin binary entry point.
- `src/cli/` owns command syntax and CLI-to-runtime adapters.
- `src/app/` owns foreground assembly, reload/restart orchestration, API setup,
  plugin runtime installation, and shutdown ordering.
- `src/build_info.rs` combines build features with plugin catalog information.
  It is intentionally outside `infra` because it depends on plugin state.

### DNS model and execution core

- `crates/proto/` owns the DNS message model and wire format.
- `src/core/` owns `DnsContext`, response classification, and reusable matching
  primitives that are independent of concrete plugin registration.
- `src/plugin/` owns extension traits, factories, dependency analysis,
  registry/runtime lifecycle, and concrete plugin categories.

### Infrastructure

`src/infra/` contains subsystem-neutral services used by more than one runtime
surface: clocks, errors, tasks, networking, observability, service management,
TTL cache primitives, line-oriented I/O, and upgrade mechanics.

The dependency rule is one-way:

```text
api / app / cli / plugin -> core / infra / proto
infra                    -X-> plugin
```

Do not introduce plugin traits, registries, tags, configuration models, or
plugin-specific metrics into `infra`. A helper qualifies for `infra` only when
its public concepts remain useful without knowing which plugin calls it.

### Management surfaces

- `src/api/` owns the management HTTP server, built-in routes, authentication,
  CORS, static WebUI serving, and plugin route registration.
- `webui/` consumes the management API and mirrors plugin configuration through
  `webui/lib/plugin-definitions/`.
- `docs/` is user-facing documentation and must not be treated as the internal
  architecture source of truth.

## Plugin Package Boundaries

Plugin-shared code should first remain inside its category:

- `plugin/server/`: connection lifetime, request handling, and server metrics.
- `plugin/matcher/rules/`: matcher config parsing, source classification,
  numeric parsing, and provider binding.
- `plugin/provider/`: provider API integration and provider-format helpers such
  as V2Ray models, parsers, and selectors.
- `plugin/executor/`: executor-specific families and integration adapters.

For a growing plugin package, keep `mod.rs` as the facade and orchestration
layer. Split stable responsibilities into names such as `config`, `model`,
`api`, `metrics`, `persistence`, `manager`, `parser`, or `transport`. Do not
split solely to reduce line count; split when a module owns an independently
testable policy or lifecycle.

### Root crate API stability

OxiDNS is maintained and released as a binary application, not as a Rust SDK.
The root `src/lib.rs` exists to share implementation with the binary, workspace
tests, and internal tooling; `pub` visibility in the root crate does not by
itself create a supported downstream Rust API contract.

- Do not add compatibility facades, deprecated aliases, or duplicate lifecycle
  paths solely to preserve old root-crate Rust imports.
- Internal refactors may rename, move, or remove root-crate items when all
  in-repository callers are migrated and the architecture improves.
- Compatibility review remains mandatory for operator-facing contracts:
  configuration, management HTTP APIs, persisted data, DNS wire behavior,
  command-line behavior, service paths, and release artifacts.
- A Rust API requires source-compatibility handling only when the project has
  explicitly documented that API as supported. Independently published
  workspace crates must define and follow their own stability policy.

Detailed plugin registration and feature rules live in `ai/plugin-dev.md`.

## Startup, Reload, and Shutdown

The normal startup sequence is:

1. Parse and validate configuration, including environment expansion and
   included files.
2. Initialize networking metrics and the optional API hub.
3. Register built-in API routes and install the application controller.
4. Build the plugin catalog, analyze dependencies, and prepare startup hooks.
5. Initialize live plugins in dependency order; unused provider chains may be
   skipped.
6. Mark health state and start the API listener.

Shutdown reverses ownership:

1. Stop accepting management traffic and clear global API access.
2. Destroy plugin instances in reverse initialization order.
3. Stop background tasks and unregister metric sources in each owner's
   `destroy` implementation.
4. Clear the application controller and runtime handles.

Reload builds a replacement runtime before making it current. Changes to
runtime swapping must preserve the last usable runtime until replacement
initialization succeeds, reject overlapping reload requests, and clean up
partially initialized resources.

## State and Concurrency Rules

- Request-local mutable state belongs in `DnsContext`; do not add global state
  for data that can be carried with the request.
- Immutable startup state should be constructed once and shared through `Arc`
  only where ownership genuinely crosses tasks.
- Global registries are lifecycle coordination points, not general-purpose
  storage.
- Background tasks must have an explicit owner, bounded work or queues, and a
  shutdown path.
- External integrations must not hold a request open unless synchronous
  completion is part of the configured correctness contract.
- Poisoned lifecycle locks may recover only where the protected operation is a
  small state swap and recovery semantics are explicit.

## Hot-Path Contract

The following paths are latency-sensitive:

- DNS decode and encode.
- `RequestHandle` and server dispatch.
- sequence execution and matcher evaluation.
- cache lookup, hit restoration, admission, and write-side maintenance.
- provider lookup and upstream selection.
- transport pool lookup and I/O dispatch.

Avoid request-time parsing, repeated normalization, unbounded iteration,
blocking file or process I/O, unnecessary cloning, high-cardinality metrics,
and locks whose critical section includes I/O or `.await`. Prepare reusable
state during plugin initialization and keep persistence, logging, and external
system reconciliation outside the response-critical path where semantics allow.

Performance work must follow `ai/performance.md`; correctness gates still
follow `ai/testing-strategy.md`.

## Architecture Change Checklist

Before merging an architectural change, verify:

- The core request path and ownership of request-local state remain clear.
- Dependency direction does not introduce `infra -> plugin` coupling.
- Root-crate Rust items do not retain compatibility facades unless they belong
  to an explicitly supported API; all in-repository callers are migrated.
- Feature-gated modules compile with the feature both enabled and disabled.
- Plugin lifecycle code owns all tasks, registrations, and teardown.
- Hot-path impact is measured when allocations, locks, cloning, parsing, or
  dispatch change.
- Required Rust, WebUI, docs, config, packaging, and release artifacts are
  identified through `ai/change-impact-matrix.md`.
- Relevant tests from `ai/testing-strategy.md` have been run and reported.
