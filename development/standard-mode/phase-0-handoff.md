# Standard Mode Phase 0 Handoff

Status: complete

Completed on: 2026-07-31

Frozen product baseline: `docs/docs/standard-mode-plan.md` at commit `90a5dec`

Phase plan: `development/standard-mode/phase-0-plan.md`

## 1. Handoff Rule

Phase 1 must read this handoff and the frozen Phase 1 requirements before its local
plan is written. The frozen Chinese and English product plans were not edited during
Phase 0 and must remain frozen in later phases. New findings and implementation
decisions belong in phase plans and handoffs.

## 2. Delivered Outcome

Phase 0 replaces the browser-owned, multi-write Standard Mode save flow with a Rust-
owned compiler and a reviewed Plan/Apply transaction. Standard Mode now has one
authoritative schema, validation result, generated runtime configuration, ownership
classification, semantic diff, and recovery path.

The implementation remains OxiDNS-native and cross-platform. It does not configure
the host DNS resolver, firewall, OpenWrt/UCI, RouterOS, `ipset`, `nftset`, or a third-
party proxy system.

## 3. Completed Work Packages

### SM-0.1 Deterministic compiler correctness

- Added the Rust `StandardIntent` schema and compiler under
  `src/config/standard_mode/`.
- Made schema version 3 the current contract and retained explicit v1 and v2
  migrations.
- Emitted the runtime cache fields `min_positive_ttl`, `max_positive_ttl`,
  `max_negative_ttl`, and `negative_ttl_without_soa`.
- Generated a distinct cache plugin and cache tag for every effective resolution
  path. Live tests prove that identical qnames on different paths do not share
  answers.
- Emitted `runtime.worker_threads` instead of the obsolete browser-generated field.
- Included effective global, path, and device filtering/query-log policy when
  planning plugins and sequences.
- Removed silent fallback for invalid upstream-group, path, rule, and device
  references.
- Mapped supported upstream strategies to native forward
  `response_selection`: `fastest`, `balanced`, `prefer_positive`, and `consensus`.
- Kept ordered fallback unavailable until it has an explicit model and rejected it
  with a blocking diagnostic.
- Removed UI controls for ECS, dual-stack selection, IP selection, query sampling,
  and scenarios. Compatibility fields remain readable, but any active unsupported
  value is rejected by the backend instead of being ignored.
- Reduced `webui/lib/standard-mode/generator.ts` to a display helper; the browser no
  longer compiles runtime YAML.

### SM-0.2 Authoritative semantic validation

- Added stable error, warning, and suggestion diagnostics with object paths.
- Validated identifiers, names, generated tags, subscription filenames, listener
  settings, TTL ranges, enabled upstreams, protocol capabilities, and all references
  representable by the current intent schema.
- Required one default upstream group and at least one enabled upstream.
- Blocked planning when a required server, executor, matcher, provider, or transport
  capability is unavailable. Missing optional metrics is reported as a warning.
- Generated candidates are preflighted through `config::validate_file`, so includes
  retain their real relative-path semantics and the final plugin graph is analyzed by
  the existing OxiDNS configuration/runtime validators.
- Frontend validation remains an editing aid only. Backend diagnostics exclusively
  decide `can_apply`.

### SM-0.3 Ownership and safe switching

- Implemented `managed`, `modified`, and `unmanaged` ownership states from current
  DNS content and stored Standard metadata.
- Added a semantic review containing preserved top-level fields and generated,
  replaced, and removed plugin tags.
- Opening or selecting Standard Mode does not write DNS YAML.
- Every Standard apply is reviewed. Modified or unmanaged configuration additionally
  requires an explicit takeover acknowledgement.
- Removed the Standard system page's reuse of Expert settings. The native Standard
  page only controls the fields owned by the Standard intent: log level and worker
  threads.
- Shared a process-wide configuration mutation lock across Standard apply, Expert
  config save, and WebUI state mutation. Expert/WebUI writes are rejected while a
  Standard transaction is pending.

### SM-0.4 Plan/Apply transaction

- Added `POST /standard/plan`, `POST /standard/apply`, and
  `GET /standard/apply/status`.
- Plan and Apply verify both DNS configuration and Standard-state base versions.
  Apply also verifies that the generated version is exactly the reviewed version.
- Apply writes an adjacent recovery journal containing the complete old and candidate
  DNS/Standard state, stages the candidate, and requests reload with a transaction
  identifier.
- Runtime assembly success finalizes Standard state and generated metadata. Assembly
  or finalization failure restores the old DNS file, old Standard state, and old
  runtime.
- Startup recovers an interrupted pending transaction before loading normal
  configuration. A corrupt journal blocks startup without modifying the active
  configuration.
- Concurrent apply and conflicting Expert/WebUI mutations are rejected.
- Atomic file replacement works on Unix and Windows; Windows uses wide paths and
  `MoveFileExW` with replace and write-through flags.
- Journal input is bounded to 2 MiB and created with mode `0600` on Unix because it
  may contain sensitive upstream configuration.
- The WebUI now performs Plan, displays ownership/diff/diagnostics, applies exact
  reviewed versions, polls transaction status across the API restart, and reloads
  authoritative state only after completion. Cancelling review performs no write.

### SM-0.5 Test baseline

- Added Rust compiler, schema migration, capability, validation, ownership,
  transaction, recovery, and conflict tests.
- Added frontend migration and Plan/Apply orchestration tests.
- Added live UDP and TCP integration tests using local mock upstreams and bounded
  timeouts.
- The UDP integration test exercises the compiled default path and a client-selected
  second path, then proves both upstream selection and path-scoped cache isolation.
- Added Chinese and English Standard Mode API documentation and synchronized WebUI
  behavior documentation/navigation.

## 4. Contract Changes

### Standard intent schema 3

- Cache fields are now `minPositiveTtl`, `maxPositiveTtl`, `maxNegativeTtl`, and
  `negativeTtlWithoutSoa`.
- Upstream strategy values are now `fastest`, `balanced`, `prefer_positive`,
  `consensus`, and the blocked placeholder `ordered_fallback`.
- v1 state migrates through v2 to v3 and reports the complete `1 -> 3` range.
- v2 cache fields migrate without losing values. Legacy parallel strategy becomes
  `balanced`; legacy sequential strategy becomes the explicitly blocked ordered
  fallback.
- Legacy inert Phase 2 values are reset to safe defaults by the v2 frontend migration
  so an old UI-only preference cannot silently change runtime behavior.
- Invalid references and duplicate identifiers are preserved during frontend
  normalization so the backend can diagnose them instead of substituting defaults.

### Backend API

- Plan is side-effect free and may inspect unmanaged configuration.
- A non-applicable Plan returns diagnostics and no generated candidate.
- Apply returns HTTP 202 with a transaction identifier after durable staging and
  reload submission, not after runtime success.
- Status is the authoritative terminal result: `pending`, `succeeded`, `failed`, or
  `recovered`.
- Stale versions, stale reviewed output, missing takeover, and a busy transaction are
  conflict responses and never partially write managed state.

### Generated ownership

- Standard Mode preserves existing `include`, `api`, `network`, and non-owned log
  settings.
- It owns `runtime.worker_threads`, `log.level`, its Standard metadata, and the
  complete generated plugin list after a confirmed takeover.
- The generated header, stable tag map, intent revision, and DNS configuration
  version form the managed-state evidence.

## 5. Frozen Exit-Gate Evidence

| Frozen Phase 0 exit gate | Evidence |
|---|---|
| UI has no setting without runtime effect | Unsupported Phase 2 controls and scenarios were removed; backend tests reject non-default compatibility values |
| Direct Standard compiler coverage | 23 focused Rust tests cover schema, compilation, validation, capability, ownership, and transaction modules; frontend contract tests are part of 105 WebUI tests |
| Stable configuration and tags | Repeated compilation equality test plus stable tag-map assertions |
| Different paths do not share cache | Compiler graph assertions and live `standard_mode_udp_paths_keep_cache_entries_isolated` test |
| Failed Apply preserves runtime/state | Runtime failure, closed reload channel, corrupt journal, and startup-recovery tests |
| Unmanaged config is not overwritten silently | Backend takeover/conflict tests and WebUI cancellation/exact-version tests |
| WebUI and Rust gates pass | Exact results are recorded below |

## 6. Verification Record

All commands were run from the repository root unless a directory is stated.

| Command | Result |
|---|---|
| `cargo +nightly fmt` | passed |
| `cargo test standard_mode --lib` | passed: 23 tests |
| `just check` | passed: formatting, all-feature Clippy with warnings denied, 1109 library tests, 4 feature-gating tests, 89 plugin integration tests, and 2 Standard Mode integration tests |
| `cargo check --no-default-features --features minimal` | passed; one pre-existing provider dead-code warning |
| `cargo check --no-default-features --features standard` | passed |
| `cargo check --target x86_64-pc-windows-gnu --no-default-features --features standard` | passed; one pre-existing Windows-only unused-import warning |
| `pnpm typecheck` in `webui/` | passed |
| `pnpm lint` in `webui/` | passed |
| `pnpm test` in `webui/` | passed: 17 files, 105 tests |
| `pnpm build` in `webui/` | passed: 14 static routes |
| `pnpm build` in `docs/` | passed: content checks and zh-Hans/en production builds; dependency emitted its existing dynamic-require warning |
| `git diff --check` | passed |

`just check-matrix` was additionally attempted. It passed the first 12 isolated
feature checks and then stopped at the pre-existing `plugin-cron`-without-`api`
failure: `register_plugin_api!` is unavailable in that feature combination. The same
unconditional registration exists at frozen baseline `90a5dec`, and Phase 0 does not
modify `src/plugin/executor/cron.rs` or the API macros. This is not a Standard Mode
regression. The required minimal, standard, full/default (`just check`), and Windows
standard boundaries all pass.

The live socket tests required execution outside the filesystem/network sandbox in
order to bind loopback UDP/TCP ports. No remote test server was required and no
external system configuration was changed.

## 7. Known Constraints and Deferred Work

- Ordered primary/secondary fallback remains intentionally unavailable until its
  explicit model is introduced in a later phase.
- ECS, ECS-aware cache keys, dual-stack selection, IP selection, query sampling, and
  scenarios are compatibility-only data in Phase 0 and are not exposed. Their active
  use remains blocked until Phase 2.
- Phase 0 supports safe review and confirmed replacement of Expert configuration; it
  does not attempt a lossy automatic import of arbitrary Expert plugin graphs.
- Standard inbound encrypted listeners, richer upstream-group lifecycle, bootstrap
  dependency planning, and full rule/exception workflows remain Phase 1 work.
- The recovery journal contains old and candidate configuration by design. Operators
  must protect the configuration directory as they protect the main OxiDNS config.
- Startup recovery is part of API-enabled builds, which includes the Standard bundle.
  Running an API-disabled binary against a directory containing a pending Standard
  transaction is outside the supported Standard Mode lifecycle.
- The unrelated isolated `plugin-cron` feature-matrix issue should be fixed in a
  separate maintenance change rather than hidden inside a Standard Mode phase.

## 8. Phase 1 Entry Checklist

Before any Phase 1 implementation:

1. Read this handoff completely.
2. Re-read frozen Phase 1 (`SM-1.1` through `SM-1.6`) and its stage gate without
   editing the frozen plan.
3. Inspect the committed Phase 0 API/schema and use them as the compatibility base.
4. Write `development/standard-mode/phase-1-plan.md` before changing product code.
5. Preserve the Rust backend as the sole compiler and Plan/Apply authority.
6. Keep host DNS, firewall, OpenWrt/UCI, RouterOS, `ipset`, `nftset`, and third-party
   integration outside Standard Mode.
7. Begin with a gap audit for upstream-group CRUD, encrypted inbound listener
   ownership, bootstrap dependency handling, rule/exception closure, and the default
   runnable Standard 1.0 experience.

Phase 1 must create its own handoff and use the `git-commit-comment` workflow before
its local stage commit.
