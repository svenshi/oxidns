# Standard Mode Phase 3 Development Plan

Status: complete

Started on: 2026-07-31

Completed on: 2026-08-01

Frozen product baseline: `docs/docs/standard-mode-plan.md` at commit `90a5dec`

Previous phase commit: `495711f`

Previous handoff: `development/standard-mode/phase-2-handoff.md`

## 1. Objective and Non-negotiable Boundary

Phase 3 delivers the frozen SM-3.1 through SM-3.4 work packages as one native
OxiDNS policy system:

```text
server -> DnsContext -> matcher/provider/sequence policy -> cache/upstream -> response
```

The Rust backend remains the sole authority for schema migration, normalization,
validation, deterministic compilation, template expansion, Plan, Apply, generated
ownership, and rollback. The WebUI edits intent and presents backend results; it
must not become a second compiler.

Phase 3 must not read, write, or depend on OpenWrt/UCI, system DNS, DHCP, firewall
rules, `ipset`, `nftset`, RouterOS, a proxy controller, or any other third-party
system. Native OxiDNS listeners, upstream transports, `outbound`, and SOCKS values
remain valid only as OxiDNS-owned inputs.

The frozen Chinese and English product plans must not be edited. Phase 2 schema v5,
stable generated tags, cache/ECS isolation, fixed priority, response-fallback truth
table, and single-record query trace are compatibility boundaries.

## 2. Entry Audit and Architecture Decisions

The repository already provides the required native primitives:

- provider: `domain_set`, `dynamic_domain_set`;
- request matchers: `qname`, `qtype`, `client_ip`, `time`, `rate_limiter`;
- response matchers: `cname`, `rcode`, `resp_ip`, `has_wanted_ans`;
- executors: `sequence`, `forward`, `fallback`, `drop_resp`, `cache`,
  `learn_domain`, ECS/dual-stack/IP-selection executors, and synthetic responses;
- server: native UDP and TCP listener plugins;
- multi-upstream consensus: native `forward.response_selection: consensus`.

The following product gaps must be closed rather than represented as inactive UI
controls:

1. Schema v5 has no dedicated-group aggregate or ownership map.
2. `dynamic_domain_set` has a bounded queue but no maximum entry count, aging,
   cleanup status, or learned/manual provenance. `learn_domain` has no live
   pause/resume control.
3. Current routing conditions cannot express time, response CNAME/RCODE, or rate
   limits, and response-time rerouting needs a non-recursive path variant.
4. `routing.scenarios` is a Phase 0 placeholder and is intentionally rejected. A
   real template must expand server-side into complete, reviewable objects.
5. Standard Apply owns configuration/state files, but does not yet track
   Phase-3-created dynamic data and metadata files for successful deletion.

Architecture decisions:

- advance Standard intent to schema v6 and migrate v5 without activating any new
  behavior;
- model a dedicated group as one self-contained aggregate, not a loose set of
  browser-created references;
- enhance the generic dynamic-domain plugins with backward-compatible optional
  lifecycle controls, then compile Standard learning profiles to those primitives;
- represent request-time and response-time advanced rules explicitly in the same
  backend PolicyPlan and generated tag map;
- expand templates through a backend preview endpoint and feed the expanded intent
  through the normal Plan/Apply transaction;
- keep response rerouting finite by attaching it only to a declared source-path
  variant; target variants never re-enter the same response rule.

## 3. Schema v6 Contract

### 3.1 Dedicated resolution groups

Add `dedicatedGroups`, where every item owns:

- stable `id`, display name, description, enabled flag, and ordered priority;
- one domain `ruleProvider` containing manual rules;
- one embedded upstream group with strategy and enabled upstreams;
- one complete path policy: filtering, query logging, independent cache, ECS,
  dual-stack, and IP selection;
- an optional native listener with address plus UDP/TCP switches.

Dedicated IDs own all derived plugin identities. An enabled group compiles to:

```text
standard_dedicated_provider_<id>
  -> standard_dedicated_match_<id>
  -> standard_dedicated_forward_<id>
  -> path-local selectors/cache
  -> standard_dedicated_path_<id>
  -> optional standard_dedicated_udp/tcp_<id>
```

The optional listener enters the dedicated path directly. It does not claim to
intercept host traffic and does not run the global classifier first. It still uses
the group's explicit filtering/query-log/cache settings and the shared structured
execution-recording contract.

Validation rejects empty enabled rule sets, invalid domains, absent enabled
upstreams, unsupported protocols/features, duplicate IDs, generated-tag
collisions, listener collisions with the main listener or another dedicated group,
and a listener with both transports disabled.

Deleting the aggregate removes every generated provider, matcher, path, selector,
cache, forwarder, listener, and tag-map row on the next successful Apply. Dedicated
groups use inline manual provider rules in Phase 3 and therefore create no hidden
rule file.

### 3.2 Bounded native dynamic learning

Add `dynamicLearning.profiles`. Each profile declares:

- stable ID, name, enabled/paused state, target path, and learned-route priority;
- response classification: QTYPEs, accepted RCODEs, wanted-answer requirement, and
  optional response-IP provider role;
- rule kind (`full` or `domain`), maximum entries, entry TTL, cleanup interval,
  queue size, batch size, and flush interval;
- failure policy: `continue` by default or explicit `fail_closed`;
- OxiDNS-managed rule and metadata paths derived only from the profile ID.

Generic `dynamic_domain_set` evolves compatibly with optional:

- `max_entries` with deterministic reject-new behavior at capacity;
- `entry_ttl_seconds` and `cleanup_interval_seconds`;
- `metadata_path` storing learned timestamp and origin (`learned` or `manual`);
- status counters for total, learned, manual, expired, capacity rejected, queue
  rejected, last success, and last error.

Omitting the new fields preserves the current Expert-mode behavior. Hot-path
matching remains an `ArcSwap` snapshot read with no file I/O or metadata parsing.
The single writer owns rule and metadata mutation, uses atomic replacement for
full rewrites, and performs bounded cleanup outside the request path.

Manual API additions are marked as manual corrections and do not expire unless the
caller explicitly requests learned semantics. Existing add/delete/clear endpoints
remain compatible. `learn_domain` gains status plus pause/resume endpoints backed by
an atomic enabled flag; pause prevents new learning but never disables matching of
the current snapshot.

Standard compiles response classification with native `resp_ip`, `rcode`, and
`has_wanted_ans` matchers before invoking `learn_domain`. Default `continue` uses
asynchronous bounded enqueue and cannot change the DNS response. Explicit
`fail_closed` uses synchronous bounded execution and is the only mode allowed to
surface a learning failure on the DNS chain.

Learned-route matcher entries execute after manual allow/block, device policy,
dedicated policy, and manual forced routing. They execute before semantic/unknown
routing, satisfying the frozen lower-priority rule.

### 3.3 Advanced rules in one PolicyPlan

Add `advancedRules` with a stable ID, name, enabled flag, priority, evaluation
phase, condition list, action, and failure policy.

Request-phase conditions support AND composition of:

- time periods with explicit IANA timezone, start/end, weekdays, and month days;
- QTYPE values;
- rate-limit exceeded with bounded QPS, burst, and IPv4/IPv6 masks;
- the existing domain/client conditions where composition is useful.

Response-phase conditions support AND composition of:

- source path ID;
- CNAME domain expressions;
- RCODE values;
- wanted-answer and response-IP provider checks where required.

Request actions may select a path or return the configured block response.
Response actions may reroute to a different path. A response reroute compiles as a
path-local native fallback around an isolated target variant:

- `fail_open`: target failure preserves the original response from the fallback
  secondary context;
- `fail_closed`: target failure returns the explicit configured RCODE;
- target variants do not contain the originating response rule, preventing loops.

Rate-limit matching uses the inverse of the native allowed-token matcher for the
exceeded branch. All matcher expressions are emitted as native sequence match lists,
and all effective decisions append structured events to the existing query trace.

Multi-upstream consensus remains the existing `StandardUpstreamStrategy::Consensus`
compiled to native `forward`. Phase 3 adds precise validation (at least two enabled
upstreams), UI explanation, golden output, and live disagreement/negative-consensus
tests; it does not add another consensus engine.

### 3.4 Complete server-side scenario templates

Remove the deferred persisted `routing.scenarios` placeholder during v5-to-v6
migration. Inactive placeholders disappear with a migration diagnostic; an enabled
legacy placeholder remains an error so no guessed policy is activated.

Add a backend template preview operation which accepts a template kind, parameters,
base intent version, and expected Standard version. It returns:

- a complete proposed schema-v6 intent;
- exact objects added and modified;
- collision and capability diagnostics;
- the normal generated config, stable tag map, semantic diff, and preflight result.

No template writes files or state during preview. The accepted proposed intent is
applied only through the existing exact-version transactional Apply endpoint.

Initial complete templates:

1. `low_latency`: target provider, dedicated upstreams using latency-aware
   selection, IP selector, cache, path, route, and explanation tags;
2. `privacy_dns`: target provider, encrypted upstream group, ECS removal, isolated
   cache/path, strict failure policy, and route;
3. `internal_domains`: target provider, internal upstream group, cache/path, route,
   and optional native listener;
4. `regional_upstream`: target provider, regional upstream group, ECS/cache/path,
   route, and explanation tags.

Every generated object uses an operator-selected namespace. A collision is an
error; templates never overwrite or silently merge an existing same-ID object.

## 4. Ownership and Transaction Rules

Schema v6 Standard state records a sorted `managedFiles` manifest for dynamic rule
and metadata files only. Paths must remain below the Standard data directory and
must be derived from validated IDs; arbitrary deletion targets are forbidden.

Plan reports candidate-created, retained, and orphaned managed files. Apply rules:

1. stage and validate config plus Standard state as today;
2. start/reload the candidate runtime;
3. finalize config/state history only after runtime success;
4. delete only manifest entries owned by the previous Standard state and absent
   from the candidate, after validating the exact allowed directory and filename;
5. report cleanup failure without deleting any unowned file or rolling back an
   already healthy DNS runtime;
6. expose retryable cleanup diagnostics and never use recursive directory removal.

Dynamic files are operational state, not configuration history. Restoring an older
intent may recreate an empty learned set; it must never resurrect a deleted or
stale learned decision implicitly.

## 5. Fixed Priority and Explanation Extension

The Phase 2 priority remains stable and is extended only at declared slots:

1. local hosts/records/redirect;
2. hard block;
3. manual allow/skip filtering;
4. DDNS;
5. device policy;
6. dedicated-group policy;
7. manual forced routing and request-phase advanced rules;
8. dynamic learned routes;
9. semantic roles;
10. unknown mode;
11. response validation, response-phase advanced rules, and fallback;
12. response post-processing and the single recorder.

Plan analysis reports category, effective order, duplicate/overridden rule, source
and target paths, template origin, owned resources, and failure policy. Query
explanation adds dedicated group, learned profile, advanced rule/evaluation phase,
rate-limit decision, response reroute, preserved-original/fail-closed branch, and
template origin while retaining all Phase 2 fields.

Counterfactual explanation (why an arbitrary rule did not match) remains Phase 4
and must not be pulled into this phase.

## 6. Implementation Work Packages

### WP-3A — Schema v6, migration, and ownership model

- Add dedicated groups, dynamic profiles, advanced rules, managed-file manifest,
  and server-side template request/response models.
- Implement v5-to-v6 migration with no behavior activation.
- Synchronize Rust and TypeScript types/defaults/normalization/validation.
- Extend capabilities and stable tag-map/summary contracts.

### WP-3B — Dedicated groups and native listeners

- Compile aggregate provider, embedded upstream, path bundle, cache, selectors,
  route matcher, and optional listeners.
- Enforce listener collision and full create/reference/delete ownership.
- Add WebUI creation/edit/delete review with affected-object preview.

### WP-3C — Dynamic provider lifecycle and Standard learning

- Add generic capacity, provenance, aging, cleanup, status, and compatible APIs.
- Add generic live pause/resume to `learn_domain`.
- Compile response classifiers, learning side effect, learned route, managed files,
  and default fail-open behavior.
- Add WebUI status, paging, pause/resume, clear, add/remove correction, and cleanup
  diagnostics through generated tags only.

### WP-3D — Advanced rules

- Compile time, QTYPE, rate-limit, CNAME, RCODE, consensus validation, and explicit
  failure policy through native matchers/executors.
- Implement non-recursive response reroute variants and structured events.
- Add one unified advanced-rule editor and Plan conflict/coverage review.

### WP-3E — Complete scenario templates

- Implement backend-only deterministic expand/preview.
- Add four complete templates and collision diagnostics.
- Add WebUI parameter forms, exact object diff, accept-to-draft, then normal
  Plan/Apply review.

### WP-3F — Documentation and operational parity

- Update non-frozen Chinese/English Standard API, WebUI, plugin, and scenario docs.
- Document dynamic file ownership, cleanup retry, pause semantics, failure modes,
  listener scope, and template collision behavior.
- Keep README capability claims native and third-party independent.

## 7. Verification Gates

Focused verification must precede the full gate:

- schema v6 default, migration, normalization, deterministic serialization, and
  Rust/TypeScript parity;
- dedicated aggregate golden output, listener collision checks, UDP/TCP live query,
  and create/reference/delete regeneration with no tag/cache/file residue;
- dynamic provider capacity, queue saturation, batch flush, metadata persistence,
  restart, aging, bounded cleanup, pause/resume, manual correction, status, and
  fail-open/fail-closed tests;
- priority tests proving learned rules cannot override manual allow/block or forced
  routing;
- time/QTYPE/rate-limit request rules and CNAME/RCODE response reroute tests,
  including original-response preservation and fail-closed behavior;
- native consensus tests for positive, matching negative, and disagreement cases;
- each template: collision test, golden config, object diff, and real query test;
- Plan/Apply exact-version, history, rollback, managed-file cleanup, cleanup failure,
  and path-safety tests;
- WebUI typecheck, lint, unit tests, and production build;
- Chinese and English documentation builds;
- `just check` and `just check-matrix`;
- Windows Standard check and Linux x86_64 musl Standard release;
- credential-free local and independently uploaded Linux isolation harness.

The Phase 3 isolation harness must use a unique
`/tmp/oxidns-standard-phase3.*` directory, verify artifact hashes, bind only
loopback listeners, install no service, change no system resolver/firewall/DHCP or
third-party state, terminate every child process, and leave its result JSON/logs for
inspection. The user's standing authorization applies; no repeated upload approval
is required.

## 8. Phase 3 Exit Audit

Phase 3 is complete only when all rows are passed:

| Frozen requirement | Required direct evidence | State |
| --- | --- | --- |
| SM-3.1 dedicated groups | Complete aggregate compilation, optional listener runtime, and create/reference/delete residue test | Passed |
| SM-3.2 native learning | Capacity, aging, cleanup, pause, inspection, correction, persistence, and failure-policy runtime tests | Passed |
| SM-3.3 advanced rules | Time/CNAME/RCODE/rate/QTYPE/consensus/failure-policy golden and live tests in one PolicyPlan | Passed |
| SM-3.4 templates | Every template has object diff, collision protection, golden config, and live query test | Passed |
| Main-path safety | Learning failure leaves DNS healthy unless explicit fail-closed | Passed |
| Priority safety | Dynamic rules remain below manual allow/block and forced routing | Passed |
| Ownership safety | Deletion leaves no generated cache, listener, tag, or managed dynamic file | Passed |
| Product boundary | Generated graph and harness contain no platform or third-party integration | Passed |
| Cross-platform/full gates | Rust/WebUI/docs/matrix/Windows/musl gates pass | Passed |
| Independent isolation | Credential-free Phase 3 server harness passes with exact cleanup | Passed |
| Frozen-plan immutability | Both frozen plans have empty diff against `90a5dec` | Passed |
| Handoff and commit | Phase 3 handoff exists and `git-commit-comment` is used for the local stage commit | Passed |

## 9. Handoff Rule

After every gate passes, create
`development/standard-mode/phase-3-handoff.md` containing the schema/API/tag-map
contract, generic plugin compatibility changes, ownership model, template inventory,
test and isolated-runtime evidence, limitations deferred strictly to Phase 4, and
the Phase 4 entry checklist.

Then read and use the `git-commit-comment` skill, stage only Phase 3 work, create the
local Conventional Commit, verify a clean worktree, and only then begin Phase 4 by
reading the Phase 3 handoff first.
