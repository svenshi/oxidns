# Standard Mode Phase 2 Handoff

Status: complete

Completed on: 2026-07-31

Frozen product baseline: `docs/docs/standard-mode-plan.md` at commit `90a5dec`

Phase plan: `development/standard-mode/phase-2-plan.md`

Previous handoff: `development/standard-mode/phase-1-handoff.md`

Phase commit: pending the required `git-commit-comment` workflow at handoff time

## 1. Handoff Rule

Phase 3 must read this handoff, the Phase 2 plan, and frozen SM-3.1 through SM-3.4
before writing its own local plan or changing product code. The frozen Chinese and
English product plans were not edited during Phase 2 and remain authoritative.

Phase 2 implements upstream-policy leak prevention inside OxiDNS. It does not claim
host-wide interception: traffic which bypasses an OxiDNS listener, hard-coded IP
connections, and encrypted DNS sent to another resolver remain outside Standard
Mode ownership. Later phases must preserve that boundary and must not add OpenWrt,
UCI, DHCP, firewall, system-resolver, RouterOS, `ipset`, `nftset`, or third-party
proxy-controller coupling.

## 2. Delivered Outcome

Phase 2 delivers native, explainable smart routing for domestic, remote, and unknown
domains. Standard intent schema 5 compiles semantic rule data, independent path
bundles, response validation, explicit fallback policy, ECS, dual-stack behavior,
IP selection, and fixed-priority routing into the existing OxiDNS plugin graph.

The Rust backend remains the only authority for migration, normalization,
validation, deterministic compilation, Plan, exact-version transactional Apply,
ownership, rollback, and generated tags. The WebUI edits this intent, surfaces
backend diagnostics/status, and resolves runtime objects only through the generated
tag map.

Strict-remote unknown queries compile to a remote-only path with no domestic or
default edge. A live failure-path test proves that a failed remote query does not
execute either domestic or default upstream. Compatibility-first starts domestic
and validates its address result before remote fallback. Privacy-first starts remote
and permits a domestic fallback only when the normalized intent explicitly enables
it.

## 3. Completed Work Packages

### SM-2.1 Semantic rule datasets

- Added the six fixed roles `domestic_domains`, `foreign_domains`, `domestic_ips`,
  `direct_domains`, `remote_domains`, and `ddns_domains` with domain/IP family
  validation.
- Each role accepts independently enabled manual rules, operator-owned local files,
  HTTP(S) subscriptions, or local native `geosite.dat`/`geoip.dat` sources with
  selectors. No filename, URL, selector, or geographic dataset is assumed.
- Sources compile to stable aggregate `domain_set`/`ip_set` providers. Native data
  sources use `geosite`/`geoip`; subscriptions receive stable download, cron, job,
  generated filename, reload, and provider tags.
- Plan rejects invalid/duplicate source IDs, bad family/source combinations, empty
  active sources, invalid rules/URLs/intervals/selectors, missing capabilities, and
  locally verifiable missing files.
- The routing UI manages every source kind and combines download/cron/provider state
  with `maxAgeHours` to show missing, stale, last-success, and last-error status.
  Failed refresh continues to retain the last successful file and live provider
  snapshot through the existing atomic download/provider swap contracts.

### SM-2.2 Domestic and remote paths

- Domestic and remote roles target separate resolution-path instances, forwarders,
  selectors, and cache plugins.
- Domestic A/AAAA responses are classified through `qtype`, `rcode`,
  `has_wanted_ans`, `cname`, `resp_ip`, and the configured domestic IP provider.
- Reasoned `drop_resp` nodes classify `domestic_ip_mismatch`, `cname_only`,
  `nodata`, `nxdomain`, and `servfail` before a native fallback can select remote.
- `fallback` now distinguishes threshold timeout, executor/transport error, and a
  completed branch without response. The three switches default to the legacy
  enabled behavior for Expert configurations and compile explicitly from Standard
  response policy.
- Ordered upstream groups compile to stable single-upstream forwards and explicit
  fallback nodes; ordered fallback is no longer represented as concurrency one.
- Live UDP and TCP tests cover valid domestic address, address mismatch, NODATA,
  NXDOMAIN, SERVFAIL, CNAME-only, no-response timeout, and valid non-address
  outcomes.

### SM-2.3 Unknown-domain modes and leak prevention

- Added compatibility-first, privacy-first, and strict-remote normalized modes.
- Every mode/path/destination compiles a stable independent cache namespace. Mode
  changes cannot reuse semantically incompatible cache instances.
- Live UDP/TCP tests directly prove the initial path for all three modes.
- Privacy-first domestic fallback exists only when explicitly enabled.
- Static graph, local failure-path, and independent Linux runtime checks prove that
  strict-remote unknown handling contains and executes no domestic/default edge.
- The query recorder now wraps the complete Standard main sequence once. Standard
  uses reserved internal marks plus generic opt-in `include_marks`/`exclude_marks`
  filters to retain per-path query-log overrides while producing one record with
  the complete initial-path, validation, fallback, and final-path trace.

### SM-2.4 ECS, dual stack, IP selection, and DNSSEC safety

- ECS supports inherit, remove, preserve client input, derive a bounded subnet from
  the client, and fixed preset address/prefix modes.
- ECS handling runs before cache/forward; all ECS-bearing path caches set
  `ecs_in_key: true`, while cache-key tests prove subnet separation.
- `prefer_ipv4`/`prefer_ipv6` are emitted as path-local continuation executors before
  cache/forward. IPv4-only and IPv6-only use independent QTYPE NODATA policy.
- `ip_selector` remains separate from upstream racing and dual-stack suppression,
  runs as a return-path processor before cache/forward, and owns path-local probe
  and selection state.
- Standard exposes only DNSSEC-safe `reorder_only` and `skip` policies; no unsafe
  RRset truncation control exists.

### SM-2.5 Fixed priority and explanation

- The compiler emits the frozen order: local answers; hard block; allow/skip;
  DDNS; device policy; dedicated exception path; forced route; semantic role;
  unknown mode; response validation/fallback; post-processing/recording.
- Rules retain source order within a category. Plan `details.ruleAnalysis` reports
  effective, duplicate, and overridden/unreachable rows with the winning rule ID;
  diagnostics distinguish duplicates from conflicting actions.
- Generic fallback preserves losing primary decision events when secondary wins.
  `drop_resp` and `fallback` add stable opt-in decision/fallback outcomes.
- The Standard query explainer uses the final matched rule and final executed path,
  while also exposing semantic role, initial path, validation result, fallback
  reason/branch, final path, final upstream, and raw events. It reports cache
  `checked`, not an unproven cache hit.

## 4. Contract Changes

### Standard intent schema 5

- Added `ruleData` with six typed roles and four source variants.
- Added `smartRouting` with domestic/remote path IDs, unknown mode, explicit privacy
  fallback, threshold, and the full response-policy truth table.
- Replaced inactive ECS and IP-selection placeholders with structured policies and
  activated every dual-stack enum value.
- Schema v4 migrates deterministically to v5 without activating deferred behavior.
  Legacy placeholders produce explicit diagnostics and preserve inactive semantics.
- Rust and TypeScript defaults, migration, normalization, validation, and types are
  synchronized; browser code remains an editor mirror rather than a compiler.

### Generated Plan and tag map

- `ruleData` maps each active semantic role to its aggregate provider tag.
- `ruleDataSources` maps `role:source` to stable download, cron, and job tags.
- `smartRouting` maps semantic matchers, path variants, validation roles, and final
  actions; `caches` contains path/mode/destination namespaces.
- Generation summary adds `ruleDataSourceCount` and `smartRoutingEnabled`.
- `plan.details.ruleAnalysis` exposes effective/duplicate/overridden rules and their
  winners. Existing Plan and Apply endpoints remain unchanged.

### Generic native plugin compatibility

- `fallback` adds `fallback_on_timeout`, `fallback_on_error`, and
  `fallback_on_no_response`, all defaulting to `true`. It preserves selected and
  failed-branch execution events and records a stable branch/reason outcome only
  when execution recording is enabled.
- `drop_resp` accepts an optional lowercase machine-readable `reason`; omitted
  configuration keeps the original clear-response behavior.
- `query_recorder` accepts optional `include_marks` and `exclude_marks`; exclusion
  wins, empty lists preserve prior record-all behavior, and filtering occurs only
  after the downstream continuation completes.
- Conditional API registration imports in cron/download and provider/app guards were
  corrected so minimal, Standard, and feature-by-feature builds remain valid.

## 5. Truth Table and Cache Invariants

| Primary result | Default Standard action | Stable reason/evidence |
| --- | --- | --- |
| Wanted A/AAAA in domestic IP provider | Accept domestic | `domestic_ip_valid` matcher evidence |
| Wanted A/AAAA outside domestic IP provider | Drop and remote fallback | `domestic_ip_mismatch` |
| CNAME-only A/AAAA response | Drop and remote fallback | `cname_only` |
| NOERROR NODATA | Drop and remote fallback | `nodata` |
| NXDOMAIN | Drop and remote fallback | `nxdomain` |
| SERVFAIL | Drop and remote fallback | `servfail` |
| No response by threshold | Remote fallback when enabled | `timeout` |
| Executor/transport failure | Remote fallback when enabled | `transport_failure` |
| Wanted non-address answer | Accept initial path without IP geography | Address validation is not applied |

The explicit response-policy booleans may replace a default drop/fallback with
fail-closed/accept-initial behavior, but strict-remote validation never permits a
domestic/default edge.

Cache invariants are fixed:

1. ordinary paths, smart semantic paths, unknown modes, primary/fallback
   destinations, and DDNS each own distinct tags;
2. ECS-altering paths which retain/add ECS always key by ECS scope;
3. device policies select a complete path bundle rather than sharing a cache across
   different path semantics;
4. IP-selector and dual-selector state is path-local;
5. DDNS bypasses ordinary cache and uses an explicit short TTL.

## 6. Verification Record

| Command or gate | Result |
| --- | --- |
| `just check` | passed: all-feature format/Clippy plus 1134 library, 4 feature-gating, 89 plugin integration, and 10 Standard integration tests |
| `just check-matrix` | passed: 42 feature checks, minimal/Standard/all-feature Clippy and test suites |
| focused Standard integration | passed: 10 tests, including live UDP/TCP truth table and all unknown modes |
| Windows GNU Standard check | passed; one pre-existing conditionally unused archive import warning |
| Linux x86_64 musl Standard release | passed; static PIE SHA-256 `ce6fc293dcae2fbe036a4e3600bebd4d261826167595eff7a65bc8cf242bf2ed` |
| WebUI typecheck / lint / test / build | passed: 19 files, 112 tests, and 16 static routes |
| Chinese and English documentation build | passed |
| local credential-free isolation harness | passed: 16/16 checks in `/private/tmp/oxidns-standard-phase2-local.I02cV5/phase2-e2e-result.json` |
| frozen-plan diff against `90a5dec` | passed for both Chinese and English files |
| `git diff --check` | passed |

The independently authorized Linux server run used only
`/tmp/oxidns-standard-phase2.ce6fc2/`, loopback listeners, two OxiDNS mock
upstreams, and a local Python rule server. It installed no service and changed no
system DNS, firewall, OpenWrt/UCI, DHCP, or other application state.

Both uploaded artifacts matched local hashes: the Standard binary hash above and
credential-free harness SHA-256
`d85f423d3be0492f763ad5478ac1bc37eb8874ff23fe94637971a2cf0ab452cf`.
All 16 remote checks passed, including schema v5 Plan/Apply, native graph boundary,
ECS-safe caches, manual/local/subscription roles, provider lifecycle, domestic
validation/fallback, complete structured query trace, unknown cache isolation, and
strict-remote routing. Compatibility config version was
`f08ef82e242023f36d38b2ab5c1dfd81fa41b88cb379839af00c98e0665d75ca`;
strict config version was
`5d6e079462d2213d13ef5b2f2b58905602debb32a1531a5427f19a16fa3fbf60`.
The harness terminated every process and the final exact-path process query returned
no result. Logs and the result JSON remain in the authorized temporary directory.

## 7. Product Boundary and Phase 3 Deferred Work

- Leak prevention applies only after a query reaches OxiDNS. Phase 2 does not own
  host routing, client DNS enforcement, application interception, or another DNS
  endpoint.
- Semantic roles are source-agnostic policy inputs, not an embedded geographic
  truth set. Operators remain responsible for their chosen data and its licensing.
- Subscription stale state is reported by the WebUI from runtime file status and the
  configured age budget; no external monitoring service is introduced.
- Phase 2 does not add dedicated resolution-group lifecycle, optional per-group
  native listeners, dynamic learning, advanced time/CNAME/RCODE/rate-limit/QTYPE
  rules, or complete scenario templates. These are frozen SM-3.1 through SM-3.4.
- Phase 2 query explanation covers executed decisions. Counterfactual explanation,
  such as why a rule did not match, remains frozen Phase 4 work and must not be
  silently pulled into Phase 3.

## 8. Phase 3 Entry Checklist

Before Phase 3 implementation:

1. Read this handoff and `development/standard-mode/phase-2-plan.md` completely.
2. Re-read frozen SM-3.1 through SM-3.4 and the Phase 3 exit gate without modifying
   either frozen plan.
3. Inspect committed schema v5, generated tag map, fixed-priority compiler, cache
   namespace rules, fallback truth table, and single-record query-trace contract as
   compatibility boundaries.
4. Write `development/standard-mode/phase-3-plan.md` before any Phase 3 product-code
   change.
5. Define complete create/reference/delete ownership for dedicated groups, including
   generated files, listeners, caches, and tag-map entries.
6. Define bounded dynamic-learning capacity, queueing, persistence, aging, cleanup,
   pause, inspection, correction, and failure policy before exposing controls.
7. Keep dynamic rules below manual allow/block and forced-routing priority.
8. Define advanced-rule composition and explanation through the same PolicyPlan;
   do not create a second browser-side compiler.
9. Require each scenario template to generate a complete reviewable object set with
   collision diagnostics, golden config, and real query tests.
10. Preserve the native, cross-platform, third-party-independent product boundary.
11. Use the user's standing authorization for future isolated artifact validation,
    while retaining a unique `/tmp/oxidns-standard-phase3.*` directory,
    credential-free harness, checksum verification, loopback-only listeners, and
    process cleanup.
12. Phase 3 must create its own handoff and use `git-commit-comment` before its local
    stage commit.
