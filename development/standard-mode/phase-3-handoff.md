# Standard Mode Phase 3 Handoff

Status: complete

Completed on: 2026-08-01

Frozen product baseline: `docs/docs/standard-mode-plan.md` at commit `90a5dec`

Phase plan: `development/standard-mode/phase-3-plan.md`

Previous handoff: `development/standard-mode/phase-2-handoff.md`

Phase commit: pending the required `git-commit-comment` workflow at handoff time

## 1. Handoff Rule

Phase 4 must read this handoff, the Phase 3 plan, and frozen SM-4.1 through SM-4.3
before writing its local development plan or changing product code. The frozen
Chinese and English product plans were not edited during Phase 3 and remain the
authoritative product contract.

Standard Mode remains an OxiDNS-native control plane. It does not read, write, or
depend on OpenWrt/UCI, operating-system DNS, DHCP, firewall rules, `ipset`,
`nftset`, RouterOS, a proxy controller, or any other third-party system. Native
listeners only serve clients which explicitly reach OxiDNS; no host-wide
interception is claimed.

## 2. Delivered Outcome

Phase 3 advances Standard intent from schema 5 to schema 6 and delivers the frozen
SM-3.1 through SM-3.4 scope as one deterministic native policy plan:

```text
server -> DnsContext -> matcher/provider/sequence policy -> cache/upstream -> response
```

The Rust backend remains the sole authority for migration, normalization,
validation, compilation, tag ownership, template expansion, Plan, exact-version
Apply, rollback, and managed-file cleanup. The WebUI edits schema-v6 intent and
uses generated metadata; it does not compile an independent plugin graph.

Phase 3 adds complete dedicated resolution groups, bounded dynamic learning,
request/response advanced rules, and four complete server-side templates. The same
candidate graph was exercised locally and on an independently uploaded Linux
Standard bundle with live UDP and DoT queries.

## 3. Schema v6 and Compilation Contract

### Dedicated resolution groups

- `dedicatedGroups` owns stable ID, rules, embedded upstream strategy/members,
  path-local policy, priority, and an optional native UDP/TCP listener.
- Every enabled aggregate compiles a provider, matcher, upstream group, path entry,
  optional cache/selectors, and optional listeners under ID-derived stable tags.
- Main-listener queries enter the fixed classifier order; a dedicated listener
  enters only its own complete path and never claims host traffic interception.
- Validation rejects empty rules/upstreams, unsupported protocols, invalid IDs,
  generated-tag collisions, listener collisions, and listeners with no transport.
- Removing an aggregate regenerates without its provider, matcher, upstream, path,
  cache, listener, or tag-map entry. Inline dedicated rules create no hidden file.

### Bounded native learning

- `dynamicLearning.profiles` owns classification, target path, learned-route
  priority, rule kind, capacity, TTL, cleanup cadence, bounded queue/batch/flush,
  pause state, and explicit `continue` or `fail_closed` policy.
- Standard generates only the exact managed files
  `./data/standard-dynamic-learning/<id>.rules` and `<id>.meta.json`.
- Response classifiers use native QTYPE, RCODE, wanted-answer, and optional
  response-IP matchers before `learn_domain`.
- Learned routing runs below manual allow/block, device/dedicated policy, manual
  forced routing, and request advanced rules, but above semantic/unknown routing.
- Default asynchronous `continue` cannot alter the DNS response. Only explicit
  synchronous `fail_closed` may surface a learning failure.

### Advanced rules

- `advancedRules` supports AND-composed request conditions for time, QTYPE,
  rate-limit exceeded, domain, and client IP.
- Response conditions support source path, CNAME, RCODE, wanted-answer, and
  response-IP role checks.
- Request actions select a complete path or return the configured block response.
  Response actions reroute once through an isolated target variant.
- `fail_open` preserves the original response when the target fails;
  `fail_closed` returns an explicit RCODE. Target variants never contain the
  originating response rule, so rerouting cannot recurse.
- Native forward consensus remains the only consensus engine and requires at least
  two enabled upstreams.

### Server-side templates

`POST /api/standard/templates/preview` accepts a schema-v6 base intent, template
kind, namespace, domains, upstreams, optional listener, and optimistic versions.
It performs no write and returns the proposed complete intent, object diff,
explanation tags, normal diagnostics, generated YAML, tag map, and semantic Plan.

The complete templates are:

1. `low_latency`: fastest upstream strategy, IP selection, isolated cache/path,
   provider, route, and explanation tags;
2. `privacy_dns`: encrypted upstreams only, ECS removal, strict isolated path and
   cache;
3. `internal_domains`: internal authority path with optional native listener;
4. `regional_upstream`: regional upstream policy with explicit ECS and isolated
   cache/path.

An existing object with the selected namespace is a hard collision. Preview never
overwrites or silently merges user resources. Accepted drafts still use the normal
Plan/Apply transaction.

## 4. Generated Metadata and API Contract

Schema-v6 `tagMap` extends the existing contract without changing prior keys:

- `dedicatedGroups[id]`: `provider`, `matcher`, `upstreamGroup`, `path`, `entry`,
  optional `cache`, `udpListener`, and `tcpListener`;
- `dynamicLearning[id]`: `provider`, `learner`, `matcher`, `action`, `rulesPath`,
  and `metadataPath`;
- `advancedRules[id]`: final native action tag;
- `managedFiles`: sorted exact dynamic rule/metadata paths;
- generation summary: dedicated-group, dynamic-profile, and advanced-rule counts.

Generic runtime APIs remain plugin-tag scoped:

- `dynamic_domain_set` retains add/delete/clear compatibility and exposes status,
  paging, learned/manual provenance, capacity rejection, aging, and cleanup data;
- manual additions are non-expiring corrections unless learned origin is
  explicitly requested;
- `learn_domain` exposes status plus `POST /pause` and `POST /resume`; pause stops
  new learning without disabling the current provider snapshot.

The Standard API adds only the backend template preview route. Existing Plan,
Apply, status, history, and restore endpoints retain their exact-version and
transaction semantics.

## 5. Generic Plugin Compatibility Changes

- `dynamic_domain_set` adds optional `max_entries`, `entry_ttl_seconds`,
  `cleanup_interval_seconds`, and `metadata_path`. Omitting them preserves Expert
  Mode behavior.
- Matching remains an `ArcSwap` snapshot read with no file I/O, metadata parsing,
  or unbounded lock on the request hot path. One bounded writer serializes file,
  metadata, capacity, expiration, and correction updates.
- Rule and metadata rewrites use atomic replacement. Restart restores learned and
  manual provenance; only learned entries expire.
- `learn_domain` adds a backward-compatible optional initial `paused` state and
  atomic runtime controls.
- `black_hole` adds explicit SERVFAIL synthesis used by fail-closed policy while
  retaining existing defaults.

## 6. Ownership and Transaction Contract

- Standard state records a sorted `managedFiles` manifest containing only
  ID-derived dynamic-learning rule and metadata files.
- Plan reports created, retained, and orphaned files.
- Cleanup occurs only after the candidate runtime succeeds and only for exact
  previous-manifest paths absent from the candidate.
- A path must match the fixed `./data/standard-dynamic-learning/<safe-id>` rules or
  metadata form. Cleanup is never recursive and never accepts an arbitrary path.
- Cleanup failure is reported for retry without rolling back an already healthy DNS
  runtime or touching an unowned file.
- Dynamic operational data is not configuration history. Restoring older intent
  may recreate an empty learned set but never resurrects stale learned decisions.

## 7. WebUI and Documentation

- Added `/standard/advanced` with full dedicated-group create/edit/delete,
  dynamic-learning configuration and runtime controls, unified advanced rule
  editing, and four template parameter/diff/accept flows.
- Rust and TypeScript schema-v6 types, defaults, normalization, validation, tag map,
  API models, query-explainer indexes, and localized strings are synchronized.
- Query explanation recognizes dedicated group, learned profile, request/response
  advanced rule, response reroute, preserved-original/fail-closed branch, and
  template origin while retaining the Phase 2 trace.
- Chinese and English Standard API, advanced-policy, dynamic provider,
  observability, and response-executor documentation are synchronized.

## 8. Verification Record

| Command or gate | Result |
| --- | --- |
| `just check` | passed: format, all-feature Clippy, 1150 library, 4 feature-gating, 89 plugin integration, and 12 Standard integration tests |
| `RUST_TEST_THREADS=1 just check-matrix` | passed: 42 feature checks plus minimal, Standard, and all-feature Clippy/test suites; serialization is required because existing tests share global outbound state |
| focused lifecycle and policy tests | passed: dedicated create/delete/listener, learning capacity/expiry/provenance/restart/pause/correction/error modes, priority, every advanced condition, finite response reroute, and consensus cases |
| WebUI gates | passed: typecheck, lint, 19 files/112 tests, production build with 17 static routes |
| documentation gates | passed: Chinese and English production builds |
| Windows GNU Standard check | passed; one pre-existing conditionally unused archive import warning |
| Linux x86_64 musl Standard release | passed; static PIE SHA-256 `c37b1bcf29420c0a294725b1ec0c106e48323e2b2b38df175422ebfcddc6125f` |
| frozen-plan diff against `90a5dec` | passed for both Chinese and English files |
| `git diff --check` | passed |

The credential-free isolation script SHA-256 is
`d042471c62b66441dee895a92653f590d9a332b1c815b10842dab30f37ff43f6`.
It binds only loopback sockets, generates a transient one-day self-signed DoT
certificate at runtime, embeds no credential, installs no service, changes no
system resolver/firewall/DHCP or third-party state, terminates every child, and
removes transient TLS key material.

Local acceptance passed all four templates. Its result remains at
`/tmp/oxidns-standard-phase3.UhSNUJ/result.json` with SHA-256
`03d10da74508c09b29a31e399c13c2142b9a1de39879ec499a323b9cdc7c243e`.

The independently uploaded Linux run used only
`/tmp/oxidns-standard-phase3.UhSNUJ/` on `172.16.2.55`. Uploaded binary and script
matched `SHA256SUMS`; every template passed preview, complete object diff, config
validation, and a real dedicated-listener query. `privacy_dns` reached a real
loopback DoT server; the other templates reached loopback UDP upstreams. Collision
protection, forbidden integration scanning, and exact child cleanup passed, and
stderr was empty. The retrieved result is
`/tmp/oxidns-standard-phase3.UhSNUJ/remote-result.json` with SHA-256
`ac36e8100e1b43cee2fa13d643265059abdd211cbeee01f2e2d7b96b89f4c93a`.

## 9. Phase 3 Exit Audit

| Frozen requirement | Direct evidence | Result |
| --- | --- | --- |
| SM-3.1 dedicated groups | aggregate golden/delete test plus live main and dedicated UDP listener integration | Passed |
| SM-3.2 native learning | bounded provider and learner unit/API/restart/failure-policy tests | Passed |
| SM-3.3 advanced rules | every declared condition compiles; finite response reroute and native consensus tests pass | Passed |
| SM-3.4 templates | four deterministic previews, collision test, config check, and local/remote live query | Passed |
| Main-path safety | paused/default-continue learning preserves DNS flow; explicit fail-closed is isolated | Passed |
| Priority safety | compiler test proves manual forced routing precedes learned routing | Passed |
| Ownership safety | create/delete regeneration, exact manifest path validation, post-success cleanup | Passed |
| Product boundary | generated graphs and isolation scans contain no platform/third-party integration | Passed |
| Cross-platform/full gates | Rust/WebUI/docs/matrix/Windows/musl gates above | Passed |
| Independent isolation | credential-free uploaded Linux harness, four live queries, exact cleanup | Passed |
| Frozen-plan immutability | empty diff for both frozen plans against `90a5dec` | Passed |

## 10. Limitations Deferred Strictly to Phase 4

Phase 3 explanation describes executed decisions. It intentionally does not yet
provide counterfactual rule evaluation, cache miss causality, upstream exclusion
timing, per-stage latency decomposition, or a trace-to-intent navigation surface.

Phase 3 also does not add Standard intent import/export, explicit migration
preview, semantic asset diff beyond the current Plan summary, Expert read-only
analysis, Standard-to-Expert copy, or saved user templates. Existing history and
restore remain transactional building blocks, not the complete Phase 4 asset UI.

These are frozen SM-4.1 through SM-4.3. Phase 4 must implement them without adding
unbounded request logging, high-cardinality metrics, browser-side compilation, or
third-party template infrastructure.

## 11. Phase 4 Entry Checklist

Before Phase 4 implementation:

1. Read this handoff and `development/standard-mode/phase-3-plan.md` completely.
2. Re-read frozen SM-4.1 through SM-4.3 and its exit gates without modifying either
   frozen plan.
3. Audit the current Plan details, semantic diff, generated tag map, dependency
   graph serializer, query recorder/event model, history/restore, and WebUI asset
   stores as compatibility boundaries.
4. Write `development/standard-mode/phase-4-plan.md` before any Phase 4 product-code
   change.
5. Define a bounded diagnostic record and trace-to-intent identity contract before
   adding counterfactual explanations or latency fields.
6. Define canonical import/export envelopes, migration preview, semantic diff,
   Standard-to-Expert copy, Expert read-only analysis, and saved-template ownership
   before exposing asset controls.
7. Preserve schema-v6 stable IDs and all existing tag-map keys unless a new schema
   migration is explicitly justified.
8. Plan local and independently uploaded Linux isolation using a new unique
   `/tmp/oxidns-standard-phase4.*` directory and the standing authorization.
