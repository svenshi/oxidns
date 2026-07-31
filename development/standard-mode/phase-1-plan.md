# Standard Mode Phase 1 Development Plan

Status: complete

Started on: 2026-07-31

Completed on: 2026-07-31

Frozen product baseline: `docs/docs/standard-mode-plan.md` at commit `90a5dec`

Previous handoff: `development/standard-mode/phase-0-handoff.md`

## 1. Phase Objective

Deliver the frozen Standard 1.0 scope in SM-1.1 through SM-1.5 as a stable,
cross-platform, OxiDNS-native DNS product. A Standard Mode user must be able to
manage upstream groups, compose resolution paths, apply filtering and local DNS
policies, inspect runtime behavior, and safely roll back configuration without
editing or understanding an Expert Mode plugin graph.

The Rust backend remains the only authority for the Standard intent schema,
normalization, semantic validation, runtime compilation, Plan review, and Apply
transaction. The WebUI edits intent and presents backend results; it does not compile
runtime YAML.

## 2. Frozen Scope and Non-goals

This phase implements only the frozen requirements:

- SM-1.1 complete upstream-group management;
- SM-1.2 complete resolution-path bundles;
- SM-1.3 filtering and subscription lifecycle;
- SM-1.4 OxiDNS-native local resolution;
- SM-1.5 runtime operations and diagnostics.

The following remain outside Phase 1:

- OpenWrt, UCI, host resolver, DHCP, firewall, system DNS, or platform package
  operations;
- RouterOS, `ipset`, `nftset`, proxy-controller, or other third-party-system
  control;
- the Phase 2 DNS-leak policy, domestic/foreign smart routing, ECS, dual-stack
  selection, IP-set selection, scenarios, encrypted inbound listeners, and
  subscription market;
- a claim that Standard 1.0 automatically understands geographic or service
  traffic.

The Phase 0 handoff's reference to SM-1.6 and encrypted inbound ownership is not part
of the frozen Phase 1 source and is treated as a handoff-note error, not a new
requirement.

## 3. Read-only Gap Audit

### 3.1 Existing foundations to retain

- Backend-owned schema v3, deterministic compiler, semantic validation, Plan/Apply,
  exact-version conflict checks, ownership classification, crash recovery, and
  per-path caches are complete.
- Path and routing-rule CRUD, filtering/cache/query-log inheritance, exception and
  device policy pages, query flow explanations, upstream metrics, provider status,
  download status, cron status, query-recorder APIs, and cache APIs already exist.
- Native `hosts`, `redirect`, `arbitrary`, `ttl`, `qtype`, `black_hole`, `download`,
  `reload_provider`, `cron`, cache, and query-recorder plugins cover the required
  runtime primitives.
- Download writes use a temporary file and replace the target only after a complete
  successful write. The AdGuard provider builds a replacement snapshot before
  swapping it, so a parse failure keeps the last live snapshot.

### 3.2 Gaps and correctness defects

- The DNS page edits only the default group's upstream list. Group create, edit,
  delete, copy, default selection, and reference-aware deletion are absent.
- Standard upstream intent and the test API omit timeout, pool, pipeline, outbound,
  SOCKS5, and bootstrap-version fields supported by native `UpstreamConfig`.
- The routing page checks only routing rules when deleting a path. Exception and
  device references are not listed, and group/path/rule associative navigation is
  incomplete.
- Every generated subscription cron job currently calls one shared downloader that
  downloads all subscriptions. The UI also looks for a non-existent global refresh
  job. Independent intervals and per-subscription runtime status therefore do not
  work.
- The generated filtering chain needs regression coverage for single-pass blocking,
  explicit-allow priority, and provider reload failure. Local rule files and the
  native NODATA response are not exposed.
- Hosts, redirects, local zone records, response TTL clamps, QTYPE policy, and DDNS
  cache-bypass/short-TTL policy have no Standard intent or Standard UI.
- Query history has view/filter/detail and explanation, but no clear or export
  workflow. Cache APIs have no Standard operations page.
- Existing configuration history is raw-YAML browser history. Its rollback path
  bypasses the Standard intent transaction and can turn a managed Standard config
  into modified/unmanaged state.

## 4. Product and Architecture Decisions

### 4.1 Standard intent schema v4

Introduce schema v4 with an explicit v3-to-v4 migration. Existing v1 and v2 states
must migrate through every intermediate version and report the original-to-current
range. Rust and TypeScript contracts change together.

Add these intent capabilities:

- upstream connection controls: bootstrap IP family, native outbound profile
  reference, direct SOCKS5 value, request timeout, idle timeout, minimum/maximum
  pool size, and pipeline enablement;
- local filter files alongside online subscriptions;
- NODATA as a block response;
- an OxiDNS-native local-policy bundle for hosts entries/files, redirects/files,
  local zone records/files, global response TTL bounds, QTYPE response policy, and
  DDNS domain/path/short-TTL policy.

Compatibility-only Phase 2 fields remain readable but inactive and blocked when
enabled. Schema v4 must not silently activate them.

### 4.2 Stable generated graph and priority

Use stable tags derived from validated IDs. The compiled request order relevant to
Phase 1 is:

1. local hosts and local zone answers;
2. redirect wrapping of downstream resolution;
3. explicit hard-block exceptions;
4. explicit allow/skip-filtering exceptions;
5. DDNS cache-bypass path with short response TTL;
6. device/client path selection;
7. ordinary user routing rules;
8. default path;
9. response TTL clamp and explanation metadata.

Each path keeps its own cache. `ttl` runs after forwarding has produced a response;
the surrounding cache continuation ensures cached and returned responses observe
one defined TTL policy. DDNS uses a no-cache path and a dedicated short-TTL wrapper.

### 4.3 Subscription runtime model

Compile one download executor and one cron job per enabled online subscription. A
job downloads only its own target and reloads the shared validated provider after a
successful download step. Stable per-subscription tags are exposed in the tag map so
the UI can show and refresh each source independently.

The shared AdGuard provider consumes manual rules, enabled local files, and every
download target. Explicit allow rules are normalized as AdGuard exception rules and
retain priority over ordinary block rules. Download failure keeps the old file;
provider reload failure keeps the old in-memory snapshot. Runtime status reports the
last download success/error, file metadata, cron timing, reload error, and compiled
rule counts.

### 4.4 Runtime operations and rollback

Add a Standard operations surface that resolves runtime plugin tags only from the
backend-generated tag map:

- inspect, clear, dump, and load every path cache;
- inspect upstream runtime metrics;
- inspect provider/download/cron/reload status;
- clear query history and export the currently filtered result set as JSON or CSV;
- retain query/path/rule/upstream/cache-hit explanations.

Standard rollback must restore a Standard intent through the same backend Plan/Apply
transaction; it must never write historical generated YAML directly. Add bounded,
backend-owned Standard history records adjacent to the managed Standard state. A
history entry contains the normalized intent and exact config/state versions needed
for audit, but secrets must not be emitted in ordinary list responses. Rollback
selects a history entry, replans it against the current managed state, shows the same
review, and applies only after exact-version checks. Existing Expert raw-YAML history
continues to serve Expert Mode only.

### 4.5 Deletion and navigation semantics

- Group deletion lists every referencing path and is blocked until references are
  moved.
- Path deletion lists routing, exception, and device references and is blocked until
  all references are moved.
- Group, path, and rule cards expose stable anchors and direct links so an operator
  can move from group to its paths and from a path to its rules in the same policy
  workflow without reconstructing relationships mentally.
- Generated tag explanations are shown as diagnostic metadata; raw plugin editing is
  never required.

## 5. Ordered Work Packages

### Work package 1A — Contract and compiler correctness

1. Add schema v4 Rust and TypeScript models, defaults, migration, normalization, and
   validation.
2. Extend the tag map and generation summary for subscription and local-policy
   runtime objects.
3. Add single-pass filtering regression coverage and compile independent
   subscription download/cron chains.
4. Compile native local resolution, QTYPE, TTL, and DDNS policies with the frozen
   priority order.
5. Add deterministic compiler, reference, capability, migration, and semantic
   validation tests before UI work depends on the contract.

### Work package 1B — Upstream groups and paths

1. Complete group CRUD, copy, default selection, upstream enablement, and
   reference-aware deletion.
2. Expose every Phase 1 native upstream field and extend temporary single/group tests
   to build the same `UpstreamConfig` semantics as Apply.
3. Show build capability/protocol availability and actionable failure codes.
4. Complete path CRUD, reference inventory, stable anchors, associative navigation,
   and explanation metadata.

### Work package 1C — Filtering and local resolution

1. Add local filter-file management, NODATA, explicit allow-rule normalization, and
   per-subscription refresh/status UI.
2. Add a Local DNS page for hosts, redirects, zone records, TTL range, QTYPE policy,
   and DDNS policy using only OxiDNS-native primitives.
3. Validate files, rule syntax, QTYPE values, TTL bounds, DDNS references, and plugin
   capabilities in the backend before Apply.
4. Add focused UI tests and compiler/runtime tests for success, parse failure, stale
   file retention, provider snapshot retention, allow priority, local answers,
   redirect, QTYPE response, and DDNS cache bypass.

### Work package 1D — Operations, history, and explanation

1. Add query clear and JSON/CSV export with confirmation and bounded export reads.
2. Add path-cache list/clear/dump/load controls and surface existing upstream metrics.
3. Add backend Standard history list/rollback planning and the Standard review/apply
   workflow; prevent Standard snapshots from using Expert raw-YAML rollback.
4. Consolidate download/provider/cron/reload health and query path/rule explanations
   into Standard operations and query surfaces.

### Work package 1E — Documentation and release-quality verification

1. Synchronize Chinese/English API, configuration, Standard Mode, and WebUI docs.
2. Add unit, integration, WebUI, docs, feature-boundary, and cross-platform checks.
3. Exercise the frozen end-to-end gate with temporary ports and files: create group,
   create path, set independent cache/filtering, route a query, review/apply, confirm
   query explanation, then roll back and confirm restored behavior.
4. Use the provided test server only after local gates pass. Run a temporary OxiDNS
   process and temporary configuration directory; do not install a service, change
   host DNS/firewall, or configure any third-party system.
5. Write `development/standard-mode/phase-1-handoff.md` with exact evidence and
   remaining deferred work.
6. Use the `git-commit-comment` workflow, inspect the complete phase diff, and create
   one intentional local Conventional Commit for the completed phase.

## 6. Test and Quality Gates

Phase 1 is complete only when all applicable checks pass:

- focused Rust schema/compiler/API/runtime tests;
- live UDP and TCP Standard Mode integration tests with ephemeral ports and bounded
  timeouts;
- `cargo +nightly fmt` and `just check`;
- `cargo check --no-default-features --features minimal`;
- `cargo check --no-default-features --features standard`;
- Windows Standard feature check;
- WebUI typecheck, lint, unit tests, and production build;
- Chinese and English documentation production build;
- `git diff --check`;
- the frozen Phase 1 end-to-end gate.

The known isolated `plugin-cron`-without-`api` matrix failure from Phase 0 is not
hidden or broadened into Phase 1. If touched code changes that boundary, it must be
fixed; otherwise it remains separately documented with a baseline comparison.

## 7. Exit Criteria

Phase 1 may be handed off only when:

1. every frozen SM-1.1 through SM-1.5 bullet is either implemented with evidence or
   explicitly shown to have already been satisfied by retained Phase 0 behavior;
2. no Standard control is inert, browser-compiled, or dependent on Expert plugin
   editing;
3. subscription refreshes are independent and failure-safe;
4. local DNS and DDNS policies follow the frozen priority order and have live tests;
5. query, cache, upstream, provider, and configuration-history operations are
   available from Standard surfaces;
6. rollback uses Standard Plan/Apply and preserves managed ownership;
7. the frozen Chinese and English planning documents remain byte-for-byte unchanged;
8. the phase handoff and local commit are complete.
