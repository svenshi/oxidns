---
title: Standard Mode Product Positioning and Phased Development Plan
---

# Standard Mode Product Positioning and Phased Development Plan

## 1. Purpose

This document defines the product positioning, scope boundaries, configuration compiler architecture, implementation baseline, phased development path, and acceptance gates for OxiDNS Standard Mode.

It is a planning baseline for maintainers to split Issues, Milestones, and Pull Requests. It is not a delivery-date or compatibility commitment. Actual scheduling remains governed by project Issues, Milestones, and Releases. Changes to the product boundary, core compilation semantics, or phase objectives should be reflected here before feature development begins.

## 2. Product Positioning

### 2.1 One-sentence definition

Standard Mode is a cross-platform DNS policy control plane and configuration compiler built entirely on native OxiDNS capabilities.

It deterministically compiles user-understandable DNS intent into an OxiDNS graph of server, executor, matcher, and provider plugins, and owns validation, preview, application, explanation, and rollback.

```text
User intent
  -> normalization and migration
  -> semantic validation
  -> PolicyPlan
  -> OxiDNS PluginGraph
  -> configuration preflight
  -> atomic apply
  -> runtime explanation and rollback
```

### 2.2 Target users

Standard Mode is primarily for:

- users who want OxiDNS without directly maintaining YAML and plugin dependency graphs;
- users who need common DNS policies such as caching, filtering, routing, upstream failover, ECS, dual-stack handling, and query explanation;
- users who run OxiDNS independently on Linux, Windows, macOS, BSD, containers, NAS devices, or other environments;
- users who want to start with form-based policy and switch to Expert Mode only when necessary.

Standard Mode does not assume that OxiDNS runs on a router, or that queries arrive from any particular gateway, proxy, or operating-system network stack.

### 2.3 Core value promises

Standard Mode must satisfy all of the following:

1. **No YAML knowledge required:** users configure operational intent instead of assembling plugins manually.
2. **Predictable behavior:** every option has explicit runtime semantics; no field may save successfully without taking effect.
3. **Verifiable configuration:** references, capabilities, conflicts, DNS semantics, and resource boundaries are validated before save.
4. **Rollback-safe application:** configuration, Standard Mode state, and runtime either advance together or remain on the previous version.
5. **Explainable queries:** the product can explain which rule matched, which path ran, which cache and upstream were used, and why fallback occurred.
6. **No artificial capability loss:** Standard Mode is a high-level compiler for the OxiDNS plugin graph, not a separate reduced DNS runtime.
7. **Cross-platform and self-contained:** every Standard Mode feature depends only on public OxiDNS capabilities.

## 3. Product Boundaries

### 3.1 In scope

Standard Mode may manage:

- OxiDNS UDP, TCP, DoT, DoH, DoH3, and DoQ listeners;
- upstream servers and groups, connection settings, bootstrap, dial addresses, and native OxiDNS outbounds;
- concurrent response selection, ordered fallback, and primary/standby failover;
- positive and negative caching, lazy cache, persistence, and cache operations;
- ad filtering, allow and block rules, subscriptions, and provider lifecycle;
- hosts, redirects, local synthetic responses, TTL, and QTYPE policies;
- native domain, client IP/CIDR, QTYPE, response IP, CNAME, and RCODE rules;
- domestic, foreign, custom, and unknown-domain resolution paths;
- ECS, IPv4/IPv6 preference, IPv4-only/IPv6-only behavior, and response IP selection;
- dynamic domain learning through native OxiDNS `dynamic_domain_set` and `learn_domain` plugins;
- query logs, metrics, execution traces, configuration explanation, and diagnostics;
- Standard Mode state, configuration history, import, export, preview, apply, and rollback;
- OxiDNS log level, worker threads, API, and other native runtime settings.

### 3.2 Explicitly out of scope

Standard Mode does not execute, configure, or depend on:

- OpenWrt UCI or LuCI;
- dnsmasq, systemd-resolved, NetworkManager, or any other system DNS service;
- nftables, iptables, pf, firewall rules, or port 53 interception;
- DHCP, MAC addresses, leases, or third-party device discovery;
- changes to operating-system DNS settings;
- proprietary APIs, ports, or configuration models from Clash, sing-box, mihomo, or other proxy software;
- third-party FakeIP pools or proxy modes;
- RouterOS, ipset, nftset, or similar network side effects;
- installation, startup, shutdown, recovery, or configuration synchronization of third-party services.

These capabilities may remain available as Expert Mode plugins, independent integration projects, or external controllers, but they must never become dependencies of the Standard Mode model or lifecycle.

### 3.3 Product meaning of “DNS leak prevention”

Within these boundaries, “DNS leak prevention” in Standard Mode can only mean **OxiDNS upstream-policy leak prevention**:

> OxiDNS does not send a protected class of domains, including unknown domains, to an upstream group forbidden by policy, and it does not silently cross into a forbidden path when a protected path fails.

It does not mean:

- intercepting client DNS traffic that never reaches OxiDNS;
- preventing browsers or applications from using their own DoH;
- modifying system routes or firewalls;
- automatically controlling third-party proxies.

WebUI copy, help, and diagnostics must state this boundary clearly. “Strict remote resolution” must not be presented as machine-wide or network-wide DNS leak prevention.

## 4. Relationship with Expert Mode

Standard Mode and Expert Mode share one OxiDNS runtime but expose different configuration entry points:

- Standard Mode maintains structured intent and owns the plugins and files generated from that intent;
- Expert Mode permits direct editing of complete YAML and arbitrary plugin graphs;
- Standard Mode may expose “View generated YAML” and “Copy as Expert configuration”;
- Expert configuration cannot be automatically reverse-compiled into Standard Mode without explicit import rules;
- when entering Standard Mode from Expert Mode, existing configuration must be marked `unmanaged` and must not be overwritten without a preview;
- when an Expert Mode edit modifies generated configuration, ownership must change to `modified` and the next apply must show the diff.

Configuration ownership has at least three states:

| State | Meaning | Standard Mode behavior |
|---|---|---|
| `managed` | The current configuration was generated by this intent and versions match | Normal plan and apply are allowed |
| `modified` | Generated configuration was changed externally | Show the diff and overwrite scope before apply |
| `unmanaged` | The current configuration has never been managed by Standard Mode | Import supported parts, save separately, or explicitly confirm replacement |

## 5. Principles for Learning from Reference Projects

Standard Mode learns DNS policy compilation lessons from [sbwml/luci-app-mosdns](https://github.com/sbwml/luci-app-mosdns) and [jasonxtt/mosdns](https://github.com/jasonxtt/mosdns), without copying their platform coupling or fixed configuration layouts.

### 5.1 Designs to adopt

#### From sbwml/luci-app-mosdns

- A feature toggle must change the complete execution chain, not merely persist state.
- Domestic, foreign, and unknown domains must have explicit resolution paths.
- Domestic upstream responses can be verified with an IP provider and trigger remote fallback when invalid.
- ECS, IPv4 preference, cache, TTL, ad filtering, DDNS, and streaming paths have explicit insertion points.
- The ordering of hosts, redirects, cache, rules, routing, and fallback is product behavior.

#### From jasonxtt/mosdns

- Structural changes and lightweight policy switches should be handled separately.
- Different resolution semantics need different caches so results cannot cross paths or modes.
- A dedicated routing group should generate a complete bundle of provider, cache, upstream, sequence, and optional listeners.
- Allow lists, block lists, DDNS, dedicated groups, domestic/foreign rules, and unknown-domain handling need a stable priority model.
- Query logs should present the final effective path and upstream, not only intermediate plugins.
- Unknown domains can be learned into native OxiDNS dynamic domain sets based on response IP classification.

### 5.2 Designs not to adopt

- Shell/UCI configuration generation;
- numbered `switch1...switch17` controls;
- fixed bits, marks, slots, or reserved ports;
- operating-system-specific paths and service controls;
- FakeIP, port, or control logic specific to third-party proxies;
- deployment-system operations mixed into the DNS policy compiler.

OxiDNS should adopt the policy semantics and reimplement them with clear names and typed, migratable native models.

## 6. Current Implementation Baseline

### 6.1 Existing foundation

The current Standard Mode already provides:

- Schema v2 state and legacy-state migration;
- DNS, filtering, routing, exceptions, devices, query log, and overview pages;
- upstream address and protocol conversion;
- upstream connectivity tests;
- basic forward, path sequence, cache, filter, and server generation;
- ad-list subscription download, scheduled refresh, and provider reload;
- client IP/CIDR device policy;
- query recording plus metadata that maps standard tags to paths, rules, and upstream groups;
- build-capability awareness;
- atomic writes and version-conflict protection for Standard Mode state;
- generated configuration submission through the existing validation and runtime reload flow.

The current implementation is therefore an end-to-end skeleton, not a page prototype. The next priority is correcting compilation semantics and safety boundaries, not adding more pages.

### 6.2 Current release assessment

Standard Mode should remain **Alpha / Experimental** and should not yet be recommended by default to users with existing Expert Mode configuration.

The primary reason is that user intent, generated configuration, and runtime behavior can still diverge.

### 6.3 P0 gaps

#### Configuration takeover and transactions

- Switching from Expert Mode lacks import, diff, and destructive replacement confirmation.
- Standard state, DNS YAML, generated metadata, and runtime are committed separately instead of as one transaction.
- When generated metadata is absent, an existing configuration can be incorrectly treated as synchronized.
- Saving Standard Mode regenerates the complete plugin list while preserving only a limited set of top-level settings.
- The Standard system page reuses Expert settings that can later be overwritten by the Standard generator.

#### Compilation semantics

- The cache generator emits `min_ttl`, `max_ttl`, and `negative_ttl`, which do not match the runtime fields.
- All paths share one `standard_cache`, even though the cache key does not include path or upstream group.
- `parallel`, `fastest`, and `sequential` are not correctly compiled into `response_selection` or explicit fallback.
- Path-level filtering and query logging do not always cause the required plugins to be generated.
- `dualStack`, `ipSelection`, `ecs`, and query-log sampling exist in state but are not fully compiled.
- `runtime.threads` does not match Rust configuration's `runtime.worker_threads`.
- Missing build capabilities can be skipped silently without showing the complete degradation to the user.

#### Incomplete feature loops

- Standard pages mainly edit the default upstream group; other groups lack complete CRUD.
- Routes can reference paths and groups, but users cannot easily complete “create group -> create path -> create rule -> test.”
- Scenario templates create partial path drafts rather than complete policies matching their names.
- Per-subscription refresh intervals are collapsed into the shortest interval.
- Core Standard Mode schema, generator, migration, inheritance matrix, and interpreter lack direct automated tests.

## 7. Target Configuration Compiler Architecture

### 7.1 Authority boundary

The Rust backend should become the authority for Standard Mode normalization, semantic validation, capability planning, configuration generation, and commit.

The WebUI is responsible for:

- collecting and displaying user intent;
- local form validation and immediate feedback;
- presenting plans, diagnostics, diffs, and explanations returned by the backend;
- not independently defining final runtime semantics.

The backend is responsible for:

- schema migration and normalization;
- global reference and conflict validation;
- build and runtime capability checks;
- PolicyPlan and PluginGraph generation;
- configuration analysis, staging, switching, and runtime reload;
- generated metadata, versions, and rollback history.

Frontend and backend may share JSON Schema or generated types, but they must not maintain two independent compiler implementations.

### 7.2 Core objects

```text
StandardIntent
  listeners
  upstreamGroups
  resolutionPaths
  filteringPolicy
  routingPolicy
  exceptionPolicy
  clientPolicies
  cacheDefaults
  observability
  runtime

PolicyPlan
  normalizedIntent
  providers
  upstreamRoles
  pathBundles
  orderedRules
  requiredCapabilities
  managedFiles
  diagnostics
  ownership

PluginGraph
  plugins
  dependencies
  entrypoints
  tagMap
  config
```

`StandardIntent` is user configuration, `PolicyPlan` is an explainable policy plan, and `PluginGraph` is the runtime artifact. The WebUI should not require users to understand `PluginGraph`, but advanced users must be able to inspect it.

### 7.3 Feature compilers

Compilation should be separated into independently testable feature compilers:

```text
compileRuntime()
compileListeners()
compileRuleSources()
compileFilteringPolicy()
compileUpstreamGroups()
compileCachePolicy()
compileResolutionPaths()
compileDualStackPolicy()
compileEcsPolicy()
compileIpSelectionPolicy()
compileRoutingPolicy()
compileClientPolicies()
compileObservability()
```

Each compiler returns at least:

- generated plugins and dependencies;
- mappings from user intent to generated tags;
- required build capabilities;
- managed files and runtime resources;
- errors, warnings, and optional suggestions;
- a summary of request-hot-path impact.

### 7.4 Compiler invariants

The Standard Mode compiler must satisfy these invariants:

1. The same normalized intent and capability set produce the same configuration.
2. All references resolve before generation; invalid references never silently fall back to the first group or path.
3. A missing core capability requested by the user fails compilation instead of being skipped.
4. Only explicitly optional observability capabilities may degrade, and all degradation produces a visible warning.
5. Every Path with distinct resolution semantics uses an independent cache instance.
6. An ECS-enabled path explicitly decides whether ECS participates in the cache key.
7. Every setting exposed by the UI has a generation test proving that it changes the execution graph.
8. Generated tags are stable, unique, and traceable from runtime events to user objects.
9. Standard Mode manages only configuration nodes and files it explicitly owns.
10. Compilation and application never execute on the DNS request hot path.

## 8. Core Standard Mode Policy Semantics

### 8.1 Resolution Path Bundle

A resolution path is not merely an upstream reference. It is a complete policy bundle with the target shape:

```text
Path
  query recorder
  filtering policy
  dual-stack policy
  ECS policy
  IP selection policy
  path-scoped cache
  forward or fallback
  response TTL policy
  accept
```

Paths with different semantics must not share result-affecting state, including cache, ECS cache keys, dynamic learning sets, and probe scores. Resources may be shared only when their result semantics are proven equivalent.

### 8.2 Upstream strategies

Standard Mode should use names aligned with runtime behavior:

| User strategy | Compiled result |
|---|---|
| `fastest` | `forward.response_selection: fastest` |
| `balanced` | `forward.response_selection: balanced` |
| `prefer_positive` | `forward.response_selection: prefer_positive` |
| `consensus` | `forward.response_selection: consensus` |
| `ordered_fallback` | An ordered primary/standby chain built from one or more explicit `fallback` plugins |

`concurrent: 1` is not ordered fallback and must no longer be presented as `sequential`.

### 8.3 Upstream leak-prevention modes

Phase 2 introduces three unknown-domain policies:

| Mode | Compiled semantics for unknown domains |
|---|---|
| Compatibility first | Query the domestic path first, verify response IP, and fall back to remote on invalid response or timeout |
| Privacy first | Query the remote path first and allow domestic fallback only when there is no valid result |
| Strict remote | Use only the remote path; return failure when it fails, without domestic or default-path fallback |

Strict remote mode must validate that:

- the remote group exists and contains at least one enabled upstream;
- the path contains no domestic group reference;
- fallback contains no forbidden branch;
- encrypted upstreams addressed by domain have explicit bootstrap or dial addresses;
- an explicit outbound never silently falls back to the default outbound;
- the plan states the failure RCODE and fail-open/fail-closed semantics.

### 8.4 Rule priority

Standard Mode maintains a stable system priority:

| Order | Rule category | Meaning |
|---:|---|---|
| 1 | Local hosts / redirect | Produce local or rewritten results first |
| 2 | Forced block | Explicit hard blocks configured by the user |
| 3 | Allow / bypass filtering | Exceptions to ad and subscription filtering |
| 4 | DDNS | Bypass normal cache by default; optional short TTL and dedicated path |
| 5 | Client IP/CIDR policy | Use only source addresses actually visible to OxiDNS |
| 6 | Dedicated resolution path | High-priority groups configured by the user |
| 7 | User forced-routing rules | Evaluate in explicit user order |
| 8 | Domestic domains | Domestic path |
| 9 | Foreign domains | Remote path |
| 10 | Unknown domains | Apply the selected upstream leak-prevention mode |
| 11 | Response validation and fallback | `resp_ip`, `rcode`, `has_wanted_ans`, and similar rules |
| 12 | Response post-processing and recording | TTL, IP selection, explanation, and final acceptance |

Users may reorder rules within a category. Cross-category reordering is available only in Expert Mode. The compilation plan must display the final effective order.

## 9. Phased Development Plan

Phases are defined by completion gates, not dates. Safety and semantic requirements from an earlier phase must not be skipped to meet a version label.

### Phase 0: Trustworthy Compiler

#### Objective

Remove silent failure, cross-path cache pollution, unsafe mode takeover, and non-atomic apply risks. This moves Standard Mode from Alpha to Beta.

#### Work packages

##### SM-0.1 Fix deterministic compilation errors

- Emit the runtime fields `min_positive_ttl`, `max_positive_ttl`, `max_negative_ttl`, and `negative_ttl_without_soa`.
- Generate an independent cache for every cache-enabled Resolution Path.
- Set the correct `ecs_in_key` behavior for ECS-enabled paths.
- Generate `runtime.worker_threads` correctly.
- Fix path-level filtering and query-log plugin generation conditions.
- Reject invalid path references instead of falling back to the first upstream group.
- Map upstream strategies to actual `response_selection` or `fallback` semantics.
- Temporarily hide ECS, dual-stack, IP selection, sampling, and scenario controls that are not yet compiled.

##### SM-0.2 Unify semantic validation

- Establish one authoritative `validateStandardIntent`.
- Validate uniqueness of IDs, names, generated tags, and filenames.
- Validate group, path, rule, device, and provider references.
- Detect dependency cycles, empty upstream groups, and plugins without entrypoints.
- Validate listener conflicts and port syntax.
- Separate errors, warnings, and suggestions.
- Block apply when core capabilities are absent and remove silent `skippedCapabilities` behavior.

##### SM-0.3 Configuration ownership and safe switching

- Implement `managed / modified / unmanaged` states.
- When entering Standard Mode from Expert Mode, offer read-only analysis, supported-item import, or confirmed replacement.
- Show which configuration will be preserved, taken over, or deleted.
- Modify only top-level fields and plugins declared as Standard Mode-owned.
- Eliminate double writes between Standard and Expert system settings.

##### SM-0.4 Plan/Apply transaction

- Add backend `standard/plan`.
- Add backend `standard/apply`.
- Check the Standard state version and DNS configuration version together.
- Analyze generated configuration and initialize a replacement runtime in staging.
- Commit Standard state and generated metadata only after replacement runtime succeeds.
- Keep the previous runtime and managed state when any step fails.
- Record the most recent successful version and failed-apply diagnostics.

##### SM-0.5 Compiler test baseline

- Default-intent golden configuration.
- Output-difference tests for every field.
- Global, path, and client three-level inheritance matrix.
- Per-path cache isolation.
- ECS cache keys.
- Upstream selection and fallback semantics.
- Missing-capability matrices.
- Schema migration and damaged-state recovery.
- Expert-to-Standard switching.
- Version conflict and failed-apply rollback.
- Live UDP/TCP queries that verify the final path.

#### Exit gates

- The UI exposes no setting without runtime effect.
- Direct Standard Mode tests cover every compiler module.
- Identical input produces stable configuration and tags.
- Different paths cannot share DNS result caches.
- Apply failure changes neither the active runtime nor Standard state.
- Unmanaged configuration cannot be replaced without confirmation.
- WebUI typecheck, lint, test, and build plus the full Rust quality gate pass.

### Phase 1: OxiDNS Native Standard 1.0

#### Objective

Deliver a stable, cross-platform, self-contained base DNS product. Users can complete common resolution, filtering, caching, and routing tasks without understanding plugin graphs.

#### Work packages

##### SM-1.1 Complete upstream-group management

- Create, edit, delete, and duplicate upstream groups.
- Manage upstream membership and enabled state.
- Support upstream protocols compiled into OxiDNS.
- Support bootstrap, dial address, TLS validation, timeout, pipeline, and HTTP/3.
- Support generic OxiDNS outbound and SOCKS fields without connecting to or controlling any third-party software.
- Test individual upstreams and complete groups.
- Show build capabilities, protocol availability, and failure reasons.

##### SM-1.2 Resolution Path Bundle

- Complete path CRUD.
- Bind paths to upstream groups.
- Support independent filtering, cache, and logging inheritance.
- Compile stable path tags and explanation metadata.
- Provide linked navigation for “upstream group -> path -> rule.”
- Show all references before deleting a path.

##### SM-1.3 Complete basic filtering loop

- Manual allow and block rules.
- Local files and online subscriptions.
- Explicit allow-rule priority.
- Either refresh each subscription independently or replace the product model with one global interval.
- Keep the last successful file after download failure.
- Reload a provider only after the new file passes validation.
- Support NXDOMAIN, NODATA, NULL, and REFUSED.
- Show last success, last error, rule count, and manual refresh for subscriptions.

##### SM-1.4 Native local policies

- hosts;
- redirects;
- local synthetic responses;
- TTL ranges;
- QTYPE policies for HTTPS/SVCB, AAAA, and others;
- DDNS rules that bypass normal cache by default and allow short TTLs;
- compile all functionality only into native OxiDNS plugins.

##### SM-1.5 Operations and diagnostics

- View, clear, and export query history.
- Inspect, clear, dump, and load caches.
- Display upstream runtime statistics.
- Explain path and rule hits.
- Display provider, download, cron, and reload status.
- Provide configuration history and one-click rollback.

#### Exit gates

Users can reliably complete:

```text
Create an upstream group
  -> create a resolution path
  -> enable an independent cache and filtering
  -> add a routing rule
  -> preview the generated plan
  -> apply
  -> confirm the final path in query logs
  -> roll back
```

The default profile promises only safe, general-purpose DNS resolution, caching, and filtering. It does not claim intelligent domestic/foreign routing in this phase.

### Phase 2: Native Smart Routing and Upstream Leak Prevention

#### Objective

Use native OxiDNS matchers, providers, sequences, forwarders, fallbacks, and caches to implement explainable routing for domestic, foreign, and unknown domains.

#### Work packages

##### SM-2.1 Semantic rule datasets

- Introduce roles such as `domestic_domains`, `foreign_domains`, `domestic_ips`, `direct_domains`, `remote_domains`, and `ddns_domains`.
- Bind roles to built-in data, local files, online subscriptions, or manual rules.
- Do not depend on fixed filenames or subscription URLs.
- Produce clear diagnostics when data is missing, stale, or failed to download.

##### SM-2.2 Domestic and remote paths

- Give the domestic path independent upstreams, cache, and optional dual-stack policy.
- Give the remote path independent upstreams, cache, ECS, and outbound.
- Validate domestic responses through `resp_ip` and a domestic IP provider.
- Use `drop_resp` to trigger explicit fallback when a response violates policy.
- Define separate fallback behavior for NODATA, NXDOMAIN, SERVFAIL, CNAME-only responses, and timeouts.

##### SM-2.3 Unknown-domain modes

- Compatibility first.
- Privacy first.
- Strict remote.
- Use an independent cache or stable independent namespace for every mode.
- Never reuse semantically incompatible cache entries after a mode change.
- Explain initial path, validation result, fallback reason, and final upstream.

##### SM-2.4 ECS, dual stack, and IP selection

- Support removing ECS, preserving client ECS, deriving it from the client, and fixed presets.
- Explicitly set `ecs_in_key` on ECS-enabled caches.
- Compile `prefer_ipv4` and `prefer_ipv6` before forward according to continuation semantics.
- Implement IPv4-only/IPv6-only with QTYPE policy, separate from preference.
- Keep `ip_selector` distinct from upstream response racing and dual-stack suppression.
- Follow `reorder_only` or `skip` safety policy for DNSSEC scenarios.

##### SM-2.5 Fixed priority and conflict explanation

- Implement the system priority in section 8.4.
- Allow sorting within each category.
- Diagnose cross-category conflicts in the compiler.
- Show shadowed, unreachable, and duplicate rules in the plan.
- Explain the final effective rule rather than only the first intermediate matcher.

#### Exit gates

- Unknown domains in strict remote mode never execute a domestic or default upstream.
- A failed domestic response-IP validation reliably enters the configured remote fallback.
- Modes, paths, ECS, and client policies cannot pollute each other's caches.
- Deterministic tests cover fallback success, no response, negative response, timeout, and transport failure.
- A query can explain why it matched, why it fell back, and what was ultimately used.
- Product copy clearly states the scope of upstream leak prevention.

### Phase 3: Native Advanced DNS Policies

#### Objective

Extend native OxiDNS policy capability and scenario expression without coupling any third-party system.

#### Work packages

##### SM-3.1 Dedicated resolution groups

Each dedicated group compiles to:

```text
Rule Provider
  + manual rules
  + upstream group
  + independent cache
  + ECS / dual-stack / IP selection
  + Resolution Path
  + optional native OxiDNS listener
```

Support general scenarios such as corporate networks, streaming, education networks, privacy DNS, internal domains, and regional upstreams, without third-party-proxy-specific semantics.

##### SM-3.2 Native dynamic learning

- Store dynamic results with `dynamic_domain_set`.
- Learn domains after response classification with `learn_domain`.
- Use `resp_ip`, `rcode`, and `has_wanted_ans` to decide learning conditions.
- Bound set capacity, batched writes, and concurrent queues.
- Provide clear, pause, manual correction, and inspection actions.
- Do not permanently trust learned results by default until aging semantics exist.
- Always rank dynamic sets below manual allow/block and forced-routing policies.

##### SM-3.3 Advanced rules

- Time rules.
- CNAME and RCODE routing.
- Rate limiting.
- Multi-upstream consensus.
- Per-QTYPE paths.
- Fail-open and fail-closed policies.
- Keep all advanced rules in the unified PolicyPlan and explanation model.

##### SM-3.4 Complete scenario templates

A scenario template must generate a complete policy, never only a name or path draft. For example, a “low-latency resolution” template generates at least:

```text
Target-domain Provider
  + low-latency upstream group
  + IP Selector
  + independent cache
  + Resolution Path
  + routing rule
  + explanation tags
```

Before apply, templates show every object they add or modify and never overwrite a user resource with the same name.

#### Exit gates

- Every template has a golden configuration and live-query test.
- Dynamic-learning failures do not affect the main DNS path unless the user explicitly selects fail-closed.
- Dynamic rules have capacity limits, cleanup, and correction mechanisms.
- Dedicated-path resources can be created, referenced, and deleted completely without orphaned caches or managed files.
- Standard Mode still depends on no third-party service.

### Phase 4: Explanation, Diagnostics, and Configuration Assets

#### Objective

Elevate Standard Mode from “can generate configuration” to a native control plane that can be operated and understood over time.

#### Work packages

##### SM-4.1 Compilation explanation

- Show intent mappings to Providers, Matchers, Paths, Caches, and Upstreams.
- Show final rule priority.
- Show cache boundaries and ECS behavior for each path.
- Show every generated tag.
- Show optional capabilities absent from the current build.
- Provide read-only YAML and a plugin dependency graph.

##### SM-4.2 Query diagnostics

- Why a rule did not match.
- Why the default path was selected.
- Why fallback was triggered.
- Why the cache did not hit.
- Why ECS produced a distinct cache entry.
- Why response-IP validation failed.
- Why an upstream was excluded or timed out.
- Final RCODE, response source, and latency breakdown.

##### SM-4.3 Configuration assets

- Import and export Standard intent.
- Schema-version migration.
- Configuration version history.
- Semantic diff, not only YAML text diff.
- Copy Standard Mode state as Expert configuration.
- Read-only capability analysis of Expert configuration.
- Save and duplicate templates without integrating a third-party template service.

#### Exit gates

- Users can trace an anomalous query back to corresponding Standard intent.
- Users can determine which paths and rules a configuration change affects.
- Import, export, and migration preserve semantics and stable IDs.
- Every Standard Mode version can roll back to the most recent successful configuration.
- Diagnostics add neither unbounded logging nor high-cardinality metrics to the request hot path.

## 10. Phase Dependencies and Parallel Boundaries

```text
SM-0.1 compilation fixes ----+
SM-0.2 global validation ----+--> SM-0.4 Plan/Apply --> Standard Beta
SM-0.3 ownership ------------+
SM-0.5 test baseline --------+

Standard Beta
  -> SM-1.1 upstream groups
  -> SM-1.2 Path Bundles
  -> SM-1.3 filtering
  -> SM-1.4 local policies
  -> SM-1.5 operations and diagnostics
  -> Standard 1.0

Standard 1.0
  -> SM-2.1 semantic datasets
  -> SM-2.2 domestic/remote paths
  -> SM-2.3 unknown-domain modes
  -> SM-2.4 ECS/dual-stack/IP selection
  -> SM-2.5 priority and explanation

Phase 3 and Phase 4 modules may proceed in parallel after core Phase 2 semantics stabilize.
```

No new scenario template, dynamic learning feature, or advanced form field should be added before Phase 0 is complete. Phase 0 correctness work can proceed in parallel across the backend compiler, WebUI state migration, and tests, but all work must converge on the same PolicyPlan contract.

## 11. Testing and Quality Gates

### 11.1 Compiler unit tests

- Schema defaults, normalization, and migration.
- Input and output of each feature compiler.
- Stable tags and ordering.
- Every reference and conflict.
- Full/standard/minimal/custom capability matrices.
- Compilation errors, warnings, and suggestions.
- Managed-file and ownership plans.

### 11.2 Golden configurations

Maintain at least:

- minimal Standard configuration;
- multi-upstream selection;
- ordered fallback;
- per-path independent cache;
- filtering and allow exceptions;
- client-path override;
- domestic/remote/unknown routing;
- strict remote mode;
- ECS;
- dual-stack preference;
- dedicated resolution groups;
- dynamic learning.

Golden configurations must also pass OxiDNS backend configuration analysis; comparing serialized text is insufficient.

### 11.3 End-to-end DNS tests

Use local mock upstreams, ephemeral ports, and bounded timeouts to verify:

- which upstream was actually accessed;
- whether a forbidden upstream was accessed incorrectly;
- fallback timing;
- positive responses, NXDOMAIN, NODATA, SERVFAIL, CNAME-only responses, and timeouts;
- cache hits/misses and path isolation;
- ECS cache keys;
- IPv4/IPv6 preference and QTYPE blocking;
- behavior before and after provider reload;
- preservation of the old runtime after reload failure.

### 11.4 WebUI tests

- Mapping between form fields and StandardIntent.
- Capability-disabled states.
- Plan, diff, apply, error, and rollback flows.
- Reference protection when deleting groups, paths, and rules.
- Mode switching and unmanaged-configuration warnings.
- Query explanations and quick actions.
- Synchronized Chinese and English copy.
- Narrow-screen, desktop, light-theme, and dark-theme layouts.

### 11.5 Minimum commands for each phase

Run as appropriate for the change scope:

```bash
pnpm typecheck
pnpm lint
pnpm test
pnpm build
cargo test
cargo test --test plugin_integration
just check
```

Add `just check-matrix` for feature/capability changes and `cd docs && npm run build` for documentation-structure changes.

## 12. Release and Feature Exposure Rules

### Alpha

- The schema may still be unstable.
- The feature must be explicitly labeled Experimental.
- Expert configuration cannot be overwritten silently.
- Unimplemented fields cannot be presented as available.

### Beta

- Phase 0 is complete.
- Standard Mode configuration can be planned, applied, and rolled back safely.
- Every schema change has a migration.
- Every public setting has a compiler test.

### Standard 1.0

- Phase 1 is complete.
- The basic upstream, path, cache, filtering, and rule loop is stable.
- Documentation clearly describes the cross-platform, third-party-independent boundary.
- The default profile directly generates a runnable configuration.

### Later releases

- Smart routing, upstream leak prevention, and dynamic learning are exposed only after their respective phase gates pass.
- Version numbers do not substitute for capability maturity.
- Experimental native plugins retain their experimental status in the UI.

## 13. Product and Engineering Metrics

Standard Mode completion is not measured by page or setting count. Track the following instead.

### Product metrics

- First-apply success rate for the default profile.
- Steps from creating an upstream to completing a successful query.
- Whether users can locate configuration failures through diagnostics.
- Percentage of query records mapped to Standard rules and paths.
- Percentage of basic scenarios that still require Expert Mode.

### Correctness metrics

- Exposed-but-uncompiled field count must remain zero.
- Silent degradation count for Standard configuration must remain zero.
- Cross-path cache false-hit count must remain zero.
- State-split incidents after failed apply must remain zero.
- Forbidden-upstream access in strict remote mode must remain zero.

### Performance metrics

- A generated request path must not parse rules per request.
- Providers, matchers, and tags should be normalized and bound at startup.
- Query explanation events must be bounded.
- Learning, download, and persistence must not perform blocking I/O on the request hot path.
- New paths and observability plugins require allocation, locking, and metric-cardinality review.

## 14. Implementation Principles

1. Correct semantics and safety precede new features.
2. Every product option maps to a testable compilation rule.
3. A page cannot be the sole implementation of configuration semantics.
4. Do not introduce fixed tags, marks, slots, or filenames for reference-project compatibility.
5. Do not package third-party system control as a Standard Mode feature.
6. Generating YAML is not completion; verify real DNS behavior.
7. Do not hide invalid references or missing capabilities with silent fallback.
8. Do not share caches or mutable state across different resolution semantics.
9. Preserve the `server -> DnsContext -> matcher/executor/provider -> upstream -> response` path.
10. A phase becomes a dependency only after all of its exit gates pass.

## 15. Final Acceptance Definition

At the long-term target, a user who does not understand the OxiDNS plugin system can:

1. Create and test upstream groups.
2. Select or create resolution paths.
3. Configure cache, filtering, dual-stack, ECS, and fallback intent.
4. Add domain, QTYPE, response, and client rules.
5. Inspect an accurate policy plan and conflicts before apply.
6. Apply safely without disrupting the previous usable runtime.
7. Understand the final path, upstream, cache, and fallback reason from query logs.
8. Modify, export, migrate, or roll back Standard intent.
9. Explicitly switch to Expert Mode when arbitrary plugin orchestration is needed.
10. Complete the entire process without depending on OpenWrt, a third-party proxy, or any other external control system.

Only then does Standard Mode become the productized entry point to native OxiDNS capabilities rather than a simplified editor for Expert YAML.
