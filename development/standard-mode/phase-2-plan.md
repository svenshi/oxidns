# Standard Mode Phase 2 Development Plan

Status: complete

Started on: 2026-07-31

Completed on: 2026-07-31

Frozen product baseline: `docs/docs/standard-mode-plan.md` at commit `90a5dec`

Previous handoff: `development/standard-mode/phase-1-handoff.md`

## 1. Objective and Boundary

Phase 2 delivers OxiDNS-native smart routing and upstream-policy leak prevention.
The complete path remains:

```text
server -> DnsContext -> matcher/provider/sequence policy -> cache/upstream -> response
```

Standard Mode will compile intent only to native OxiDNS plugins. It will not read or
write OpenWrt/UCI, system DNS, DHCP, firewall policy, `ipset`, `nftset`, RouterOS, or
third-party proxy-controller state. Native `outbound` and SOCKS values are accepted
only as OxiDNS networking inputs.

“Leak prevention” in this phase means that a query which reaches OxiDNS and is
classified as strict-remote cannot execute a domestic or default upstream branch.
It does not claim to intercept applications which bypass OxiDNS, hard-coded IP
connections, encrypted DNS sent to another resolver, or operating-system traffic
which never reaches an OxiDNS listener.

The frozen Chinese and English plans must not be edited. Any conflict is resolved in
favor of frozen SM-2.1 through SM-2.5 and its stage gate.

## 2. Architecture Audit and Decisions

The repository already provides the native execution primitives required by the
phase: `domain_set`, `ip_set`, `geosite`, `geoip`, `qname`, `resp_ip`, `rcode`,
`has_wanted_ans`, `cname`, `drop_resp`, `fallback`, `ecs_handler`, `prefer_ipv4`,
`prefer_ipv6`, `qtype`, `ip_selector`, per-path `cache`, `forward`, and `sequence`.

The following gaps must be closed rather than hidden by UI controls:

1. The Phase 1 compiler rejects ECS, dual-stack, IP selection, and ordered fallback.
2. `fallback` currently treats every response, including a negative or semantically
   invalid response, as success. Validation therefore must run inside the primary
   branch and use an explicit `drop_resp` before fallback selection.
3. A losing/invalid fallback branch's execution events are currently discarded.
   Query explanation therefore cannot prove the initial path, validation result, or
   fallback reason.
4. Existing path caches are isolated by plugin instance, but currently set
   `ecs_in_key: false`; ECS-bearing paths need explicit ECS-safe cache keys.
5. `prefer_ipv4`/`prefer_ipv6` and `ip_selector` are continuation executors and must
   be emitted before cache/forward. IPv4-only/IPv6-only are QTYPE policies, not
   preference aliases.
6. `resp_ip` validates address answers but does not classify negative responses,
   CNAME-only answers, timeouts, or transport failures. The compiled branch must
   express these outcomes separately.
7. Existing exception sorting is incomplete relative to frozen section 8.4 and the
   current explainer reports the first intermediate match instead of the final
   effective decision.

## 3. Schema v5 Contract

The Rust backend remains the single authority for migration, normalization,
validation, deterministic compilation, Plan, and Apply. TypeScript mirrors the
contract only for editing and early feedback.

### 3.1 Semantic rule data

Add six named roles with a declared family:

| Role | Family | Primary use |
| --- | --- | --- |
| `domestic_domains` | domain | select domestic path |
| `foreign_domains` | domain | select remote path |
| `domestic_ips` | IP | validate domestic address answers |
| `direct_domains` | domain | explicit native direct/domestic policy |
| `remote_domains` | domain | explicit remote policy |
| `ddns_domains` | domain | DDNS cache-bypass path |

Each role may combine ordered, independently enabled sources:

- `manual`: inline domain expressions or IP/CIDR rules;
- `local_file`: operator-owned text file;
- `subscription`: HTTP(S) source with stable generated filename and update period;
- `native_dat`: local `geosite.dat` or `geoip.dat` path plus selectors.

The compiler will not ship or assume a country dataset, filename, URL, selector, or
directory outside its own generated subscription storage. “Native data” means an
OxiDNS-native `geosite`/`geoip` provider configured by the operator; it does not mean
an opaque third-party dependency.

Every role compiles to one stable aggregate `domain_set` or `ip_set` provider. Each
subscription compiles to a stable `download` + `cron` job and feeds that aggregate.
Each `native_dat` source compiles to a stable `geosite` or `geoip` provider referenced
by the aggregate. The generated tag map exposes roles and their source lifecycle
tags.

Static Plan diagnostics cover missing IDs, empty active roles, invalid family/source
combinations, invalid rules/URLs/intervals/selectors, duplicate generated targets,
missing plugin capabilities, and missing local/native-data files where the Plan host
can verify them. Runtime status combines provider reload state and download file
metadata to report missing, stale, last-success, and last-error states. A failed
refresh retains the last successful file and live provider snapshot.

### 3.2 Smart-routing policy

Add an explicit smart-routing block containing:

- `enabled`;
- domestic and remote path IDs;
- `unknown_mode`: `compatibility_first`, `privacy_first`, or `strict_remote`;
- role bindings for domestic, foreign, direct, remote, DDNS, and domestic-IP
  validation;
- fallback threshold;
- explicit response policy for NODATA, NXDOMAIN, SERVFAIL, CNAME-only, timeout, and
  transport failure.

The UI may offer safe presets, but the normalized backend intent stores the explicit
policy. `strict_remote` rejects any fallback action which points to the domestic or
default path.

### 3.3 Path transport policy

Replace the inactive Phase 1 ECS switch with an explicit policy:

- `inherit`;
- `remove`;
- `preserve_client`;
- `client_subnet` with IPv4/IPv6 prefix lengths;
- `preset` with an IP and prefix lengths.

Retain the existing dual-stack enum and activate `disabled`, `prefer_ipv4`,
`prefer_ipv6`, `ipv4_only`, and `ipv6_only`. Replace the inactive IP-selection switch
with a bounded native configuration containing selection mode, probe methods and
budgets, top-N, native outbound/SOCKS input, cache limits, and DNSSEC policy
`reorder_only` or `skip`.

Schema v4 migrates to v5 with inactive behavior preserved: inherited ECS,
dual-stack, and IP selection remain inactive unless explicitly enabled after
migration. Previously stored Phase 2 placeholder values produce a migration warning
and a deterministic v5 equivalent; migration never silently broadens routing.

## 4. Routing and Fallback Semantics

### 4.1 Fixed decision priority

The compiler emits the frozen section 8.4 order:

1. local hosts, local records, and redirect;
2. hard block;
3. allow/skip-filtering;
4. DDNS;
5. client/device policy;
6. dedicated paths;
7. user forced-routing rules;
8. domestic-domain role;
9. foreign/remote/direct role decisions;
10. unknown-domain mode;
11. response validation and fallback;
12. response post-processing and recording.

Rules keep source order within the same category. The compiler emits diagnostics and
Plan details for exact duplicates, same-condition conflicting actions, rules hidden
by an earlier broader/equal rule, and references which can never execute. A conflict
which makes strict-remote safety ambiguous is an error; explainable non-safety
overrides are warnings.

### 4.2 Unknown-domain modes

| Mode | Initial path | Permitted fallback | Cache domain |
| --- | --- | --- | --- |
| compatibility-first | domestic | remote | `unknown_compatibility` |
| privacy-first | remote | domestic only when explicitly configured | `unknown_privacy` |
| strict-remote | remote | remote-only or fail closed | `unknown_strict_remote` |

Changing mode changes generated path/cache identity. No unknown-mode cache instance
is reused across incompatible semantics.

### 4.3 Domestic response truth table

The default smart-routing preset uses this deterministic table; every row is
represented in the normalized intent and tested independently:

| Primary outcome | Validation result | Default action | Recorded reason |
| --- | --- | --- | --- |
| NOERROR with wanted A/AAAA and domestic IP | valid | accept domestic | `domestic_ip_valid` |
| NOERROR with wanted A/AAAA but no domestic IP | invalid | drop and remote fallback | `domestic_ip_mismatch` |
| NOERROR CNAME-only for A/AAAA | incomplete | drop and remote fallback | `cname_only` |
| NOERROR NODATA | negative | drop and remote fallback | `nodata` |
| NXDOMAIN | negative | drop and remote fallback | `nxdomain` |
| SERVFAIL | failure | drop and remote fallback | `servfail` |
| no response by threshold | timeout | remote fallback | `timeout` |
| executor/network error | transport failure | remote fallback | `transport_failure` |
| NOERROR wanted non-address answer | valid without IP geography | accept initial path | `non_address_answer` |

An explicit fail-closed choice may replace the fallback action for a row, but cannot
weaken `strict_remote`. The compiler emits separate matchers/sequences for response
classification; it must not infer success solely from `has_resp`.

Ordered upstream-group fallback is compiled from enabled upstream order into stable
single-upstream forwards and native fallback nodes. It covers success, no response,
threshold timeout, and transport failure. Response-policy fallback remains a path
decision and is not conflated with group member ordering.

## 5. Cache, ECS, Dual-stack, and DNSSEC Invariants

1. Every resolution path, unknown mode, fallback destination, and semantic policy
   domain owns a distinct cache plugin tag; cache plugins are never shared across
   these boundaries.
2. A cache used after `preserve_client`, `client_subnet`, or `preset` ECS processing
   sets `ecs_in_key: true`; `remove` and inactive ECS may set it false.
3. ECS processing executes before cache lookup and forward. Internally generated ECS
   is stripped from the downstream response by the native handler.
4. `prefer_ipv4`/`prefer_ipv6` is emitted before cache and forward using continuation
   semantics. Each path gets a separate selector instance so its preference cache
   cannot cross path or mode boundaries.
5. IPv4-only blocks AAAA and IPv6-only blocks A with QTYPE policy before upstream
   execution. They do not enable preference probing.
6. `ip_selector` executes before cache/forward as a return-path processor, so the
   cache retains the full upstream RRset and client response shaping occurs after
   downstream completion. Each enabled path has an independent selector instance.
7. DNSSEC-sensitive results use `reorder_only` (never remove signed RRset members) or
   `skip` (do not alter the response). No Standard setting may expose an unsafe
   truncation mode.

## 6. Structured Explanation Contract

Execution recording is opt-in and remains absent from the hot path when query
recording is disabled. When enabled, native fallback/drop-response handling will
preserve decision events from the failed primary branch and add stable events for:

- initial semantic classification and selected path;
- the final effective rule/category after priority resolution;
- response-validation result;
- fallback reason and selected branch;
- final path and final upstream group.

The query explainer will expose these fields as structured values and continue to
show raw runtime events. It must distinguish “cache checked” from “cache hit” and
must not report the first intermediate matcher as the final effective rule.

## 7. Implementation Work Packages

### WP-2A — Runtime observability and fallback correctness

- Preserve relevant failed-branch execution events when fallback selects another
  branch.
- Add stable, opt-in decision events to response dropping/fallback selection.
- Add focused tests for success, no response, invalid response, timeout, transport
  error, standby threshold, and execution-path preservation.

### WP-2B — Schema v5, migration, and validation

- Implement semantic role sources, smart-routing policy, ECS policy, active
  dual-stack, and IP-selection settings.
- Implement v4-to-v5 migration and synchronize Rust/TypeScript normalization and
  defaults.
- Validate source files, capabilities, strict-remote constraints, fallback truth
  table, plugin budgets, DNSSEC safety, references, duplicates, and conflicts.

### WP-2C — Deterministic native graph compiler

- Compile source lifecycle, role providers, ordered upstream fallbacks, path-local
  ECS/dual-selector/IP-selector/cache bundles, domestic response validation, three
  unknown modes, and fixed-priority main policy.
- Extend stable tag metadata and Plan details with semantic roles, cache namespaces,
  conflict/coverage explanations, and decision tags.
- Prove deterministic output and absence of OpenWrt/platform/third-party control
  plugins.

### WP-2D — Standard WebUI product surface

- Add semantic data-source management and status without assumed URLs/files.
- Add smart-routing mode, domestic/remote path, fallback, ECS, dual-stack,
  IP-selection, DNSSEC, and ordered-fallback controls.
- Surface backend diagnostics and Plan coverage/conflict information.
- Extend query explanation with initial/final path, validation, fallback, and final
  effective rule.
- Clearly display the upstream-policy leak-prevention boundary.

### WP-2E — Documentation and operational parity

- Update non-frozen runtime/config/plugin documentation, English equivalents, and
  canonical examples for schema v5 and generated native behavior.
- Keep README capability statements accurate and avoid claiming host-wide DNS
  interception.
- Add lifecycle/status mapping for semantic subscriptions/providers.

## 8. Verification Gates

Focused tests are required before the full gate:

- Rust schema/migration/normalization/validation/compiler golden and difference
  tests;
- fallback and execution-path unit tests;
- Standard API Plan/Apply/history compatibility tests;
- live UDP/TCP integration tests for all truth-table outcomes and three unknown
  modes;
- deterministic strict-remote graph assertion proving no domestic/default branch;
- cache isolation tests across path, mode, ECS subnet, client policy, and fallback;
- dual-stack/QTYPE/IP-selector/DNSSEC tests;
- conflict, duplicate, overridden, unreachable, and final-effective-rule tests;
- WebUI typecheck, lint, unit tests, and production build;
- Chinese and English documentation builds;
- `cargo +nightly fmt --all -- --check`, all-feature Clippy with warnings denied,
  full tests, Standard bundle, minimal compatibility, and Windows Standard check;
- Linux Standard cross-build and isolated server E2E after explicit artifact-upload
  authorization for Phase 2.

The isolated server harness must use only a unique `/tmp/oxidns-standard-phase2.*`
directory, loopback listeners, local mock upstreams/data servers, and a
credential-free uploaded script. It must not install a service or modify system DNS,
firewall, OpenWrt/UCI, DHCP, or another application.

## 9. Completion Evidence and Handoff

Phase 2 is complete only when every frozen stage-gate item has direct test or runtime
evidence, the frozen plans are unchanged against `90a5dec`, and
`development/standard-mode/phase-2-handoff.md` records:

- delivered behavior and exact product boundary;
- schema/API/tag-map changes and migration behavior;
- truth-table and cache/ECS invariants;
- local and isolated E2E evidence;
- known limitations deferred strictly to frozen Phase 3;
- Phase 3 entry checklist.

After the handoff is written, the `git-commit-comment` skill must be read and used to
generate the local Conventional Commit message. Phase 3 may not begin until the
Phase 2 commit succeeds and the worktree is clean.

## 10. Completion Audit

Audit date: 2026-07-31

This audit combines local, cross-platform build, and independently executed Linux
runtime evidence. Phase 2 may hand off only while every row remains passed.

| Frozen requirement | Current evidence | State |
| --- | --- | --- |
| SM-2.1 semantic rule datasets | Schema v5 exposes all six roles and manual, local-file, subscription, and native-data sources; compiler/provider lifecycle tests and the 16-check local harness cover manual, file, and subscription operation/status. | Locally proven |
| SM-2.2 domestic/remote paths | Compiler and live UDP/TCP integration tests cover independent paths, `resp_ip`, reasoned `drop_resp`, wanted-IP success, mismatch, NODATA, NXDOMAIN, SERVFAIL, CNAME-only, no-response timeout, and remote fallback. Generic fallback tests cover transport-error selection. | Locally proven |
| SM-2.3 unknown-domain modes | Live UDP/TCP integration directly asserts the initial path for compatibility-first, privacy-first, and strict-remote; privacy fallback requires an explicit switch; strict-remote remote failure executes neither domestic nor default upstream. Generated cache namespaces are mode/path specific. | Locally proven |
| SM-2.4 ECS, dual stack, IP selection | Compiler tests cover all active controls, `ecs_in_key`, QTYPE-only suppression, continuation ordering, path-local selectors, and DNSSEC `reorder_only`/`skip`; cache-key unit tests prove ECS subnet separation. | Locally proven |
| SM-2.5 priority and explanation | Compiler emits the frozen priority order, Plan reports effective/duplicate/overridden rules with winner IDs, and the WebUI explainer selects the last effective rule plus initial/final paths, validation, fallback reason, and final upstream. | Locally proven |
| No third-party/platform coupling | Generated-graph assertions and the local harness reject platform integration plugins and use only native OxiDNS providers, matchers, executors, listeners, and networking inputs. | Locally proven |
| Full local quality gate | `just check` passes 1134 library tests, 89 plugin integrations, and 10 Standard integrations; `just check-matrix` passes 42 feature checks plus minimal/Standard/all-feature Clippy and test suites; WebUI has 112 passing tests plus clean typecheck, lint, and production build; both documentation builds pass. | Passed |
| Cross-platform/build artifacts | Windows GNU Standard check passes; Linux x86_64 musl Standard release is static PIE with SHA-256 `ce6fc293dcae2fbe036a4e3600bebd4d261826167595eff7a65bc8cf242bf2ed`. | Passed |
| Local isolated E2E | Credential-free loopback harness passes 16/16 checks in `/private/tmp/oxidns-standard-phase2-local.I02cV5/phase2-e2e-result.json`. | Passed |
| Frozen-plan immutability | Both frozen plans have an empty diff against `90a5dec`. | Passed |
| Independent Linux isolated E2E | The separately authorized `/tmp/oxidns-standard-phase2.ce6fc2/` run matched both local SHA-256 values and passed 16/16 checks. It exercised schema v5 Plan/Apply, the native graph boundary, semantic source lifecycle, response validation/fallback, structured trace, cache namespaces, and strict-remote routing. The harness terminated all processes; the final process check was empty. | Passed |
| Handoff and local commit | `development/standard-mode/phase-2-handoff.md` records the completed contract and Phase 3 entry gate. The required commit-message skill is used only after final staging review. | Ready for commit workflow |
