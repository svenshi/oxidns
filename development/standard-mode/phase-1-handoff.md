# Standard Mode Phase 1 Handoff

Status: complete

Completed on: 2026-07-31

Frozen product baseline: `docs/docs/standard-mode-plan.md` at commit `90a5dec`

Phase plan: `development/standard-mode/phase-1-plan.md`

Previous handoff: `development/standard-mode/phase-0-handoff.md`

Phase commit: pending the required `git-commit-comment` workflow at handoff time

## 1. Handoff Rule

Phase 2 must read this handoff, the Phase 1 plan, and the frozen SM-2.1 through
SM-2.5 requirements before its own local plan is written. The frozen Chinese and
English product plans were not edited during Phase 1 and must remain frozen.

The Phase 0 handoff accidentally referred to a non-existent frozen SM-1.6 and
encrypted inbound ownership. Phase 1 followed the authoritative frozen SM-1.1
through SM-1.5 scope. Later phases must continue to resolve handoff-note conflicts in
favor of the frozen plan rather than silently adding scope.

## 2. Delivered Outcome

Phase 1 delivers OxiDNS Native Standard 1.0 as a cross-platform, self-contained DNS
product surface. A user can manage multiple upstream groups, compose resolution
paths, apply filtering and local DNS policies, inspect runtime behavior, and restore
a previous Standard intent without editing YAML or a plugin graph.

The Rust backend remains the sole authority for schema migration, normalization,
semantic validation, deterministic graph compilation, Plan review, exact-version
Apply, ownership, runtime finalization, and rollback. The WebUI edits intent and
uses backend-generated tags for runtime operations.

No Phase 1 feature configures or depends on OpenWrt/UCI, the host resolver, DHCP,
firewall rules, system DNS, RouterOS, `ipset`, `nftset`, or a third-party proxy
controller. Native outbound and SOCKS fields are values passed to OxiDNS networking;
Standard Mode does not discover, start, stop, or configure an external system.

## 3. Completed Work Packages

### SM-1.1 Complete upstream-group management

- Added group create, edit, copy, default selection, and guarded deletion.
- Group deletion lists every referencing path and remains disabled until references
  are moved. Stable anchors and links connect groups to their paths.
- Added per-upstream enablement and the native connection fields supported in this
  phase: bootstrap IP family, dial address, TLS verification, timeout, idle timeout,
  pool bounds, pipeline, HTTP/3, OxiDNS outbound, and direct SOCKS5.
- Extended both temporary single-upstream and whole-group testing to construct the
  same native `UpstreamConfig` fields used by generated Apply configuration.
- Protocol controls remain capability-aware and invalid protocol/field combinations
  produce actionable validation or runtime-test failures.

### SM-1.2 Resolution Path Bundle

- Retained complete path CRUD and added a full reference inventory across routing
  rules, exception rules, device policies, and DDNS path selection.
- Path deletion is blocked while any reference exists and the UI links directly to
  each referencing object.
- Added stable group, path, routing-rule, exception, and device anchors plus
  group-to-path and path-to-rule navigation.
- Each path continues to compile its own cache and stable diagnostic tag. Filtering,
  cache, and query-log inheritance are resolved in the backend compiler.
- Live tests prove default/secondary routing and cache isolation.

### SM-1.3 Filtering and subscription lifecycle

- Added local AdGuard-compatible rule files and the native NODATA block response.
- Manual allow rules are normalized to AdGuard exception rules and retain priority
  over ordinary block rules.
- Each enabled online subscription now compiles to its own download executor, cron
  executor, and named job with a stable entry in `tagMap.filterSubscriptions`.
- Added `download.fail_on_error` and `cron.stop_on_error`. Their defaults remain
  `false` for generic plugin compatibility; Standard subscription chains set both to
  `true`, so provider reload is skipped after a failed download.
- Existing atomic download replacement retains the previous file on transport
  failure. The provider builds a replacement snapshot before swapping live state,
  so provider parse/reload failure retains the previous in-memory rules.
- The filtering page shows per-subscription download/cron state, supports individual
  and all-source refresh, and exposes provider rule counts and errors.
- Remote live tests exercised two independent subscriptions, a local file, manual
  block, explicit allow priority, status endpoints, manual download, cron execution,
  and provider status.

### SM-1.4 OxiDNS-native local policy

- Added schema v4 `local` intent for hosts entries/files, redirect rules/files,
  arbitrary DNS records/files, response TTL bounds, QTYPE response policy, and DDNS.
- Added an independent Standard Local DNS page with frontend editing validation and
  backend semantic/capability validation.
- Compiled local responses, QTYPE blocking, response TTL, and DDNS entirely from
  native `hosts`, `redirect`, `arbitrary`, `qtype`, `black_hole`, `ttl`, `qname`,
  `sequence`, cache, and forward plugins.
- DDNS uses an explicitly selected path, bypasses ordinary path caches, and applies
  a short fixed response TTL.
- The effective order is local answers, redirect continuation, QTYPE policy,
  ordered exceptions, DDNS, device policies, routing rules, then the default path.
- Unit and live tests cover hosts, arbitrary records, redirect, HTTPS/SVCB NODATA,
  DDNS path selection, short TTL, and cache bypass.

### SM-1.5 Runtime operations and diagnostics

- Added a Standard Operations page that resolves cache instances only from the
  backend tag map and supports entry inspection/deletion, flush, dump, and load.
- Added query-history clear plus bounded, filtered JSON/CSV export. Export filenames
  are safe on Windows.
- Retained upstream runtime metrics on the Standard overview and consolidated
  provider/download/cron status with filtering operations.
- Added focused query-explanation tests and runtime evidence for the selected path,
  routing rule, upstream group, cache hit, response, and blocking outcome.
- Added a bounded backend Standard history adjacent to the DNS configuration: at
  most 20 entries and 2 MiB, atomic replacement, and Unix mode `0600`.
- Ordinary history listing returns metadata only. The full normalized intent is
  returned only when the operator explicitly selects an entry.
- Standard restore always submits the historical intent to current Plan review and
  exact-version transactional Apply. It never writes historical generated YAML.
- Expert raw-YAML history and reload actions are inaccessible while the WebUI is in
  Standard Mode; Expert behavior remains unchanged.

## 4. Contract Changes

### Standard intent schema 4

- Added explicit v3-to-v4 migration; v1 and v2 continue through every intermediate
  schema and report the complete migration range.
- Added native upstream connection controls, `filtering.localFiles`, NODATA, and the
  complete Phase 1 local policy bundle.
- Inactive Phase 2 compatibility values remain readable but still block planning if
  activated. Schema v4 does not silently enable deferred behavior.
- Rust and TypeScript defaults, migration, normalization, validation, and tests are
  synchronized.

### Generated tag map

- `filterSubscriptions` maps each subscription ID to its download tag, cron tag,
  and job name.
- `local` maps generated local matcher/executor roles to stable runtime tags.
- Generation summaries include the effective local-policy count.
- Cache, path, routing, exception, device, upstream-group, filtering, query-log, and
  system mappings from Phase 0 remain stable.

### Standard history API

- `GET /api/standard/history` lists bounded, non-secret metadata.
- `POST /api/standard/history/restore` returns a selected normalized intent for the
  existing Plan/review/Apply flow.
- A history entry is appended only after candidate runtime assembly and Standard
  state finalization succeed. Failed/recovered transactions do not remain in
  history.

### Generic scheduler compatibility

- `download.fail_on_error` makes an executor return an error when any batch target
  fails. It is opt-in and defaults to the previous continue behavior.
- `cron.stop_on_error` stops later executors in a job after the first executor error.
  It is opt-in and defaults to the previous continue behavior.

## 5. Frozen Requirement Evidence

| Frozen requirement | Evidence |
|---|---|
| SM-1.1 group CRUD, native fields, tests, capability/failure visibility | DNS page implementation, reference selectors, extended upstream API, compiler/API tests, remote group test |
| SM-1.2 complete paths, inheritance, stable tags, navigation, guarded deletion | Routing/device/exception pages, reference selectors/tests, compiler tag map, remote secondary-path trace |
| SM-1.3 manual/local/subscription filtering and failure-safe independent refresh | schema/compiler changes, download/cron error propagation tests, filtering UI, remote two-subscription and provider-status checks |
| SM-1.4 hosts, redirect, local records, TTL, QTYPE, and DDNS | schema v4, Local DNS page, compiler/live tests, remote DNS behavior checks |
| SM-1.5 query/cache/upstream/provider diagnostics and history restore | Operations/query pages, existing overview metrics, history API/transaction tests, remote cache/trace/restore checks |
| No inert or browser-compiled Standard control | backend remains the sole compiler; focused schema/compiler/UI tests and Plan preflight cover every exposed Phase 1 field |
| OxiDNS-native and cross-platform boundary | generated-graph audit, minimal/standard/Windows checks, remote Linux graph assertion; no platform or third-party control added |
| Frozen planning documents unchanged | `git diff --exit-code 90a5dec --` passed for both frozen files |

## 6. Verification Record

All local repository commands were run from the repository root unless a directory
is stated.

| Command or gate | Result |
|---|---|
| `cargo +nightly fmt --all -- --check` | passed |
| `cargo test api::standard_mode::tests --lib` | passed: 13 tests |
| `cargo test --test standard_mode_integration` | passed: 4 live DNS tests |
| `just check` | passed: all-feature Clippy with warnings denied, 1116 library tests, 4 feature-gating tests, 89 plugin integration tests, and all integration suites |
| `cargo check --no-default-features --features minimal` | passed; two pre-existing conditionally unused warnings |
| `cargo check --no-default-features --features standard` | passed cleanly |
| `cargo check --target x86_64-pc-windows-gnu --no-default-features --features standard` | passed; one pre-existing Windows archive import warning |
| `cross build --target x86_64-unknown-linux-gnu --release --no-default-features --features standard` | passed; Linux test binary SHA-256 `e308d968b685732ed1feb31e2ac6b7c294ba393701a8fb1e287c8d06d5b1a99a` |
| `pnpm typecheck` in `webui/` | passed |
| `pnpm lint` in `webui/` | passed |
| `pnpm test` in `webui/` | passed: 19 files, 111 tests |
| `pnpm build` in `webui/` | passed: 16 static routes, including `/standard/local` and `/standard/operations` |
| `pnpm build` in `docs/` | passed: content checks plus zh-Hans/en builds; dependency emitted its existing dynamic-require warning |
| `git diff --check` | passed |
| frozen-plan diff against `90a5dec` | passed for Chinese and English files |

The provided x86_64 Linux test server ran the same credential-free harness that was
first proven locally. It used only `/tmp/oxidns-standard-phase1.hhNUWA`, ports bound
to `127.0.0.1`, two OxiDNS mock upstreams, and a local Python rule server. It did not
install a service or modify system DNS, firewall, OpenWrt/UCI, or another system.

All 23 remote checks passed:

1. isolated runtime and Standard build capabilities;
2. unmanaged Plan, reviewed transactional Apply, and native graph boundary;
3. whole-group upstream test;
4. hosts, arbitrary record, redirect, UDP/TCP, and QTYPE NODATA behavior;
5. manual block, explicit allow, two independent subscriptions, and local-file
   filtering;
6. secondary-path routing, DDNS short TTL, independent caches, and DDNS bypass;
7. cache list/dump/flush/load;
8. download, cron, refresh, and provider status for both subscriptions;
9. query history plus path/rule execution explanation;
10. second Apply followed by restoring the first intent through Plan/Apply;
11. query-history clear and the paginated source used by filtered export.

The first generated config version was
`6a4f8b243864dd5d8a1c546ba642c01ea73b2b0030c54afb13d35e4d60c01ee6`.
History contained three successful entries after the second Apply and restore. The
harness stopped every temporary process; a remote `pgrep` check returned no match.
The result and logs remain in the authorized temporary directory for inspection.

## 7. Known Constraints and Deferred Work

- Ordered fallback remains blocked until Phase 2 defines negative-response,
  transport-failure, timeout, and response-validation semantics.
- Domestic/foreign/unknown roles, response-IP validation, strict remote routing,
  upstream leak prevention, ECS, dual-stack preference, and IP selection remain
  inactive Phase 2 work. Standard 1.0 does not claim intelligent geographic routing.
- Query export intentionally stops after 100,000 matching records to bound browser
  memory. Operators requiring bulk archival should use an external API workflow.
- Standard history retains at most 20 entries within a 2 MiB sensitive file. It is a
  configuration rollback facility, not an unlimited audit database.
- Local file paths are references owned by the operator. Standard Mode does not
  create, delete, or rewrite platform rule files.
- Generic `download` and `cron` behavior changes only when the new opt-in error
  flags are set; other Expert configurations retain previous semantics.
- The Phase 0 isolated `plugin-cron`-without-`api` matrix issue remains outside the
  supported Standard bundle and is not hidden by this phase. Minimal, Standard,
  full/default, Windows Standard, and Linux Standard boundaries pass.

## 8. Phase 2 Entry Checklist

Before Phase 2 implementation:

1. Read this handoff completely.
2. Re-read frozen SM-2.1 through SM-2.5 and the Phase 2 gate without editing either
   frozen planning file.
3. Inspect the committed Phase 1 schema v4, tag map, path builder, history API, and
   runtime explanation model as the compatibility base.
4. Write `development/standard-mode/phase-2-plan.md` before changing product code.
5. Audit existing native `domain_set`, `ip_set`, `resp_ip`, `drop_resp`, `fallback`,
   ECS, dual-selector, QTYPE, CNAME, RCODE, and execution-path primitives before
   deciding whether any runtime extension is required.
6. Define the complete fallback truth table for success, NODATA, NXDOMAIN, SERVFAIL,
   CNAME-only, timeout, and transport failure before compiling smart routing.
7. Define cache separation and ECS-key invariants before exposing any Phase 2 field.
8. Preserve the precise product boundary: upstream-policy leak prevention only;
   never claim interception of DNS traffic that does not reach OxiDNS.
9. Keep every semantic dataset role source-agnostic. Do not hard-code filenames,
   subscription URLs, geographic providers, or third-party systems.
10. Phase 2 must create its own handoff and use `git-commit-comment` before its local
    stage commit.
