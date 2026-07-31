# Standard Mode Phase 4 Development Plan

Status: complete

Started on: 2026-08-01

Completed on: 2026-08-01

Frozen product baseline: `docs/docs/standard-mode-plan.md` at commit `90a5dec`

Previous phase commit: `fbea1c2`

Previous handoff: `development/standard-mode/phase-3-handoff.md`

## 1. Objective and Product Boundary

Phase 4 delivers frozen SM-4.1 through SM-4.3 and closes the full Standard Mode
acceptance contract. It turns the existing deterministic compiler and transactional
Apply flow into a long-lived OxiDNS-native control plane that can explain compiled
policy, diagnose one query, manage portable intent assets, and roll back safely.

The authoritative flow remains:

```text
Standard intent -> Rust migration/normalization/validation/compiler
                -> server -> DnsContext -> matcher/executor/provider pipeline
                -> cache/upstream -> response
```

Standard Mode must not read, write, or depend on OpenWrt/UCI, operating-system DNS,
DHCP, firewall rules, `ipset`, `nftset`, RouterOS, proxy-controller APIs, or any
third-party system. Native OxiDNS listener, upstream, outbound, and SOCKS fields
remain ordinary OxiDNS-owned configuration inputs. Expert analysis is read-only
and never attempts to control an external system.

The frozen Chinese and English plans must not be edited. Phase 0 through Phase 3
schema, stable IDs, fixed priority, cache/ECS isolation, ownership, exact-version
Apply, response fallback, dynamic lifecycle, and template contracts are retained.

## 2. Entry Audit and Architecture Decisions

The repository already has the necessary foundations:

- schema-v6 Standard intent, deterministic generation, stable tag map and summary;
- backend config validation with a typed plugin dependency graph;
- exact-version Plan/Apply, crash journal, successful history, and restore preview;
- structured sequence matcher/executor/fallback events and a bounded persistent
  query-recorder queue/database;
- cache and upstream runtime decisions which are currently observable only through
  aggregate metrics or logs;
- complete server-side scenario template preview and generated ownership metadata;
- Expert configuration validation and local editor history in the WebUI.

The remaining product gaps are:

1. Plan does not expose a typed intent-to-runtime explanation or dependency graph.
2. Semantic diff compares generated tags, not stable intent objects and impact.
3. A query record is not pinned to the Standard intent revision which produced it.
4. Per-query steps are not explicitly capped and have no timing/detail fields.
5. Cache and upstream attempt decisions are missing from the single-query trace.
6. Standard intent has no portable import/export envelope or backend asset store.
7. Copy-to-Expert and Expert capability analysis are not backend, read-only
   operations.
8. Version history cannot show semantic impact between a saved version and current
   intent.

Architecture decisions:

- keep Standard intent at schema v6 because Phase 4 adds no user intent field;
- version explanation, query-diagnostic, export, and saved-template envelopes
  independently from the intent schema;
- compute `intentRevision` as SHA-256 over canonical normalized intent JSON; it is
  stable across processes and does not depend on generated YAML formatting;
- make the Rust compiler/API authoritative for explanation and semantic diff;
- record only bounded request-local diagnostic events in the existing recorder;
  never create per-query metrics, labels, or an unbounded log stream;
- add fields to the existing query-recorder SQLite tables compatibly, preserving
  Expert databases and existing records;
- store only complete schema-v6 intent or complete template parameters; never save
  half-compiled YAML fragments;
- all asset mutations use bounded, atomic, permission-restricted local files and
  optimistic versions. No template marketplace or external service is introduced.

## 3. SM-4.1 Compilation Explanation Contract

### 3.1 Typed explanation model

Every successful generated Plan exposes `explanation` schema 1 containing:

- normalized `intentRevision`;
- intent-object mappings from stable ID and intent path to generated Provider,
  Matcher, Path, Cache, Upstream, Listener, and action tags;
- final rule rows with effective ordinal, policy slot, category, stable ID,
  evaluation phase, matcher tags, action tag, and selected path;
- path boundaries with path/upstream-group tags, stable upstream member IDs,
  independent/shared cache namespace, cache enabled state, ECS mode and
  `ecs_in_key`, filtering, query-log, dual-stack, and IP-selection behavior;
- the complete sorted generated-tag inventory;
- available and missing optional build abilities plus their affected intent paths;
- managed-file ownership and generation summary.

The explanation is generated from the same typed PolicyPlan/tag registry used to
emit YAML. It is not reconstructed from labels in the browser.

### 3.2 Read-only generated graph

Plan preflight uses the in-memory backend validator and retains its typed
`dependencyGraph` in the response. The response includes:

- nodes, edges, initialization order, and sequence flows;
- read-only generated YAML and its SHA-256 config version;
- plugin count and generated tag inventory.

No API in this work package writes YAML. Existing exact-version Apply remains the
only Standard activation path.

### 3.3 Semantic diff and impact

Replace tag-only semantic diff with schema-1 object diff between the last
successfully applied normalized intent and the candidate normalized intent:

- `added`, `removed`, `modified`, and `unchanged` stable object references;
- modified field paths with redacted values where necessary;
- affected path IDs, routing/advanced-rule IDs, cache boundaries, listener IDs,
  upstream-group IDs, and managed files;
- a deterministic human-readable impact summary;
- takeover state when no trustworthy Standard baseline exists.

Generated/replaced/removed tag lists remain as compatibility fields. Object impact
is conservative: when exact reachability cannot be proven, the object and all of
its direct path consumers are reported rather than silently omitted.

## 4. SM-4.2 Bounded Single-query Diagnostics

### 4.1 Request-local event contract

Extend `ExecutionPathEvent` compatibly with optional:

- relative start offset and duration in microseconds;
- bounded structured detail using a small typed key/value map;
- stable diagnostic outcome codes.

`ExecutionPath` receives an explicit maximum event count. The default and Standard
generated limit are 512, the accepted configuration range is 32 through 4096, and
events beyond the limit increment a request-local dropped counter. Subquery append
honors the same destination bound. The record reports `stepsTruncated` and
`droppedStepCount` so absence is never presented as proof that an event did not
occur.

The recorder gains backward-compatible optional configuration:

- `max_steps`;
- bounded static `context` containing Standard explanation schema,
  `intentRevision`, and recorder role.

Keys, values, entry count, and serialized size are capped at initialization. Expert
configurations which omit the fields retain existing behavior. Existing SQLite
tables are upgraded additively and old rows remain readable with null/default new
fields.

### 4.2 Cache decisions

When request-local execution recording is enabled, the cache executor records one
bounded event for:

- fresh hit, stale hit, miss, expired item, uncacheable request, or disabled store;
- cache tag and namespace;
- whether ECS participates in the key and whether the request supplied ECS.

No cache key, qname, client address, or ECS network is added to metrics or global
logs. The UI explanation combines the event with the compiled path boundary to say
why ECS created a distinct cache item.

### 4.3 Upstream and fallback decisions

Standard-generated forward members receive stable, non-secret member tags derived
from upstream IDs. Forward records bounded attempt events with member ID, ordinal,
elapsed time, and outcome: selected, response rejected, timeout, transport error,
cancelled after winner, or unavailable. Addresses and credentials are not copied
into diagnostic details.

Fallback retains the existing primary/secondary truth events and adds elapsed
timing. Response-IP validation retains the exact matcher outcome and mapping to the
owning intent rule/path. A default-path decision is explicit when no higher rule
returns a response.

### 4.4 Backend explanation result

The query record detail exposes a schema-1 `diagnosis` assembled from recorded
facts plus the matching applied explanation revision. It answers:

- first failed condition for evaluated rules and why a rule missed;
- why the default path was selected;
- fallback trigger and chosen branch;
- cache miss/hit reason and ECS cache separation;
- response-IP validation failure;
- upstream timeout/exclusion/error and selected source;
- final RCODE, response source, total latency, and recorded stage breakdown;
- corresponding Standard intent revision and stable object IDs.

If the current explanation does not match the record revision, the API returns the
raw bounded facts and an explicit `explanationUnavailable` reason. It never maps an
old query to a newer intent by guesswork.

## 5. SM-4.3 Configuration Asset Contract

### 5.1 Portable import/export

Add a Standard asset envelope schema 1 containing:

- asset kind and schema;
- OxiDNS build/version information;
- source Standard intent schema and `intentRevision`;
- complete normalized Standard intent;
- optional name/description and export timestamp excluded from semantic identity.

Export returns this envelope without credentials beyond values already present in
the user-owned intent. Import accepts bounded JSON, decodes every supported Standard
schema through the existing migration pipeline, normalizes and validates it, and
returns migration diagnostics plus the normal Plan response. Import performs no
write; activation still requires the reviewed exact-version Apply request.

Round-trip requirements cover schemas 1 through 6, Unicode, stable object IDs,
disabled objects, ordering normalization, and rejection of malformed/oversized
assets.

### 5.2 Successful version history and rollback

History remains bounded to the 20 most recent successful configurations and adds:

- `intentRevision`, schema, label, generation summary, and semantic impact;
- a read-only compare/restore preview against current state;
- migration through the current decoder before Plan/Apply.

Rollback is still a two-step restore-preview then exact-version Apply, so a failed
candidate cannot replace the last usable runtime. Tests cover older schemas,
concurrent version conflict, failed reload, restart recovery, and retention of the
previous successful entry.

### 5.3 Standard to Expert and Expert analysis

Copy-to-Expert returns the generated YAML, config version, dependency graph,
capability summary, and a banner indicating it is a detached snapshot. It never
switches mode, writes the config, or preserves a false Standard ownership claim.

Expert analysis accepts bounded YAML and runs the normal parser, schema validator,
feature/plugin capability checks, and dependency graph builder. It reports:

- supported native capability families;
- constructs representable by Standard Mode;
- valid Expert-only plugins/graphs which require Expert Mode;
- missing build capabilities and system-integration plugins;
- explicit reasons that reverse conversion is unavailable or lossy.

It never reverse-compiles arbitrary plugin YAML into Standard intent and never
executes, applies, or probes external systems.

### 5.4 Saved templates

Add a local schema-1 Standard asset store adjacent to the OxiDNS config with:

- at most 64 entries and a 2 MiB total file limit;
- atomic writes, mode 0600 on Unix, and optimistic store version;
- stable asset ID, name, description, template kind, complete parameters,
  created/updated timestamps, and source intent schema;
- list, save, duplicate, rename/update, and delete operations;
- collision checking through the existing server-side template preview before any
  saved template is applied.

Saving and duplicating do not modify Standard intent or runtime config. Deletion is
exact-entry only and never recursive. No third-party template service is used.

## 6. API and WebUI Work

Backend routes are versioned by their envelope fields and remain under `/standard`:

- existing `/standard/plan` gains explanation, graph, and object impact;
- `GET /standard/assets/export` and `POST /standard/assets/import`;
- `POST /standard/assets/expert-copy` and
  `POST /standard/assets/expert-analysis`;
- `GET/POST/PATCH/DELETE /standard/assets/templates` plus duplicate;
- history list gains metadata and compare/restore remains read-only until Apply;
- query-recorder detail gains bounded diagnostic fields without breaking existing
  list/filter/stat endpoints.

The WebUI adds a compact Standard operations surface for:

- compilation explanation, final priority, path/cache/ECS matrix, generated YAML,
  dependency graph, missing capabilities, and semantic impact;
- query diagnosis with facts, stage timing, truncation warning, and exact intent
  revision;
- import/export, history compare/restore, copy-to-Expert, Expert read-only analysis,
  and saved-template management.

Chinese and English UI/docs describe native boundaries, read-only operations,
rollback semantics, recorder limits, privacy behavior, and the fact that a native
listener serves only explicitly directed clients.

## 7. Compatibility, Performance, and Security

- Expert configuration remains valid when all new recorder/upstream fields are
  omitted.
- No new dependency direction from `infra` to plugin code is introduced.
- Hashing, explanation construction, semantic diff, import, and graph analysis run
  only in control-plane operations, never per DNS request.
- Request-time work is conditional on the existing recorder flag and bounded by
  event count and forward fan-out. Disabled recording adds only the existing branch.
- SQLite migrations are additive, transaction-safe, and tested against legacy
  files. Retention, queue, batching, reader concurrency, and vacuum bounds remain.
- Diagnostic details never include passwords, proxy credentials, TLS secrets,
  complete cache keys, or private response payloads beyond existing query-record
  access.
- New JSON/YAML input and local asset files have strict byte/count/string limits.
- No unbounded/high-cardinality metric or log label is added.

## 8. Verification Matrix

### Compiler and control plane

- deterministic intent revision, explanation, mappings, final priority, path
  boundaries, missing capabilities, YAML and dependency graph;
- semantic object diff with stable IDs and conservative affected-resource closure;
- export/import/migrate round trips for every supported schema;
- history compare/restore and successful-runtime rollback invariants;
- copy-to-Expert and Expert analysis are read-only and bounded;
- saved-template CRUD/duplicate/version conflict/limits/collision preview;
- corrupted/truncated asset and history files fail safely.

### Runtime diagnostics

- request-local event cap and subquery append truncation;
- recorder static revision context and additive legacy SQLite migration;
- cache fresh/stale/miss/expired/ECS-key events;
- sequential/concurrent/fastest/consensus upstream outcomes with stable member IDs;
- fallback timeout/error/response-IP mismatch and default-path decisions;
- final source, RCODE, and stage timing reconstruction;
- recording disabled, queue full, restart, retention, and legacy records.

### Integration and UI

- golden config and live UDP/TCP/encrypted queries for representative Standard
  policies;
- local isolated Phase-4 harness with semantic asset round trip and rollback;
- independently uploaded Linux static binary plus credential-free loopback-only
  harness in a unique `/tmp/oxidns-standard-phase4.*` directory;
- exact child-process cleanup and scans proving no system/service/resolver/DHCP/
  firewall/third-party operation;
- WebUI typecheck, lint, unit tests, production build, and route inventory;
- Chinese and English documentation builds;
- native build/lint/test gate, serialized feature matrix, Windows check, Linux musl
  static PIE build, artifact hashes, and frozen-plan diff audit.

## 9. Phase Exit and Final Acceptance Gates

Phase 4 is complete only when:

1. one anomalous recorded query traces to its exact Standard intent revision and
   stable policy objects, or states explicitly why the matching explanation is no
   longer available;
2. Plan identifies which paths, rules, caches, listeners, upstream groups, and
   files a semantic change affects;
3. export/import/migration preserve semantics and stable IDs across all supported
   schemas;
4. every successful Standard version can be restored through Plan/Apply without a
   failed candidate replacing the last healthy runtime;
5. diagnostic capture is explicitly bounded and adds no high-cardinality metrics;
6. all ten frozen final acceptance outcomes are evidenced by automated tests,
   generated artifacts, or the completed Phase 0 through Phase 4 handoffs;
7. Standard Mode remains native OxiDNS functionality with no OpenWrt or external
   system coupling;
8. the frozen Chinese and English plans remain byte-for-byte unchanged against
   `90a5dec`;
9. the phase plan and handoff enumerate local/remote evidence, hashes, known limits,
   and the final operating contract;
10. the phase changes are committed locally using the required
    `git-commit-comment` workflow, with a clean worktree afterward.

## 10. Implementation Order

1. typed explanation, intent revision, validator graph, and semantic impact;
2. bounded execution events plus compatible recorder storage/context;
3. cache/upstream/default/fallback diagnostic facts and backend diagnosis;
4. portable assets, history metadata/compare, Expert read-only tools, templates;
5. WebUI explanation/diagnostics/assets and bilingual documentation;
6. focused tests, full gates, local and remote isolation, exit audit, handoff, and
   local commit.

## 11. Completion Record

All six implementation steps and all ten phase exit gates passed. The authoritative
evidence, artifact hashes, final acceptance mapping, and bounded limitations are in
`development/standard-mode/phase-4-handoff.md`. The frozen Chinese and English
product plans remain byte-identical to commit `90a5dec`.
