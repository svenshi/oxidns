# Standard Mode Phase 4 and Final Handoff

Status: complete

Completed on: 2026-08-01

Frozen product baseline: `docs/docs/standard-mode-plan.md` at commit `90a5dec`

Phase plan: `development/standard-mode/phase-4-plan.md`

Previous handoff: `development/standard-mode/phase-3-handoff.md`

Phase commit: this handoff's local `feat(standard-mode)` commit

## 1. Final Product Contract

Standard Mode is now the OxiDNS-native product control plane for users who should
not need to understand plugin YAML. The Rust backend remains authoritative for:

- Standard intent migration, normalization, validation, deterministic compilation,
  stable IDs, priority, ownership, semantic impact, capability checks, and graph;
- exact-version Plan/Apply, crash recovery, healthy-version history, and restore;
- request-local bounded diagnostics and intent-revision identity;
- portable intent assets, saved template assets, Expert snapshot generation, and
  read-only Expert capability analysis.

The request path remains:

```text
server -> DnsContext -> matcher/executor/provider pipeline
       -> cache/upstream -> response
```

Standard Mode does not read, write, or depend on OpenWrt/UCI, operating-system DNS,
DHCP, firewall rules, `ipset`, `nftset`, RouterOS, proxy controllers, template
marketplaces, or any other third-party control system. A native listener serves only
clients explicitly directed to it; OxiDNS does not claim host-wide interception.

## 2. Delivered Phase 4 Capabilities

### Compilation explanation and semantic Plan

- Canonical `sha256:` intent revision over normalized JSON.
- Schema-1 compiler explanation maps stable intent objects to generated Provider,
  Matcher, Path, Cache, Forward, Listener, and action tags.
- Final evaluation order, selected paths, path/upstream/member boundaries,
  cache namespaces, ECS key semantics, filtering, query logging, dual-stack/IP
  selection, generated tags, managed files, and capability gaps are explicit.
- Plan returns the backend validator dependency graph and read-only generated YAML.
- Semantic diff reports added/removed/modified/unchanged stable objects, changed
  field paths, and affected paths, rules, caches, listeners, upstream groups, and
  managed files. Legacy tag fields remain compatible.

### Bounded query diagnosis

- Execution events have optional microsecond offset/duration and a bounded detail
  map; each request defaults to a hard 512-event limit and reports truncation.
- Recorder configuration accepts bounded `max_steps` and static revision context.
- SQLite upgrades are additive; old tables and rows remain readable.
- Cache events identify fresh/stale/miss/expired/unavailable decisions and ECS-key
  participation without exporting cache keys, client networks, or credentials.
- Forward events identify stable member ID, attempt ordinal, selected/rejected/
  timeout/error/cancelled outcome, and duration. Fallback selection has elapsed
  timing and retains its exact branch/reason. Fallback branch merging counts only
  the branch-local truncation delta instead of duplicating inherited drops.
- Record detail returns schema-1 diagnosis: intent revision, rule miss/default path,
  fallback, cache, upstream attempts/failures, final response facts, total/stage
  timing, and truncation. The browser refuses to map a historical record through a
  newer intent revision and shows raw facts instead.

### Assets, history, and Expert bridge

- Schema-1 bounded Standard intent export/import envelope; import migrates through
  every supported intent schema and returns normal Plan without writing.
- History entries include intent revision and generation summary; restore remains a
  read-only preview followed by exact-version Apply.
- Standard-to-Expert returns detached YAML, graph, version, revision, and capability
  facts without changing ownership or mode.
- Expert analysis validates bounded YAML read-only, identifies native capability
  families, Expert-only objects and system integration plugins, and explicitly
  rejects lossy reverse compilation.
- Local saved templates are bounded to 64 entries/2 MiB, written atomically with
  restrictive Unix permissions, use optimistic versions, and support save, update,
  duplicate, and exact-entry delete. No remote template service exists.

## 3. API, WebUI, and Documentation

The Standard API adds export/import, Expert copy/analysis, and saved-template routes
under `/standard/assets`. Existing Plan now includes explanation, dependency graph,
and object impact; history and query detail responses gain compatible metadata.

The WebUI adds:

- semantic impact, compiled priority, path/cache/ECS matrix, capability gaps,
  generated YAML and dependency graph to Apply review;
- backend query diagnosis, exact revision, truncation warning, and raw facts;
- intent import/export, detached Expert copy, read-only Expert analysis, and local
  saved-template management.

All new UI strings and operational documentation are synchronized in Chinese and
English. See `docs/docs/standard-mode-operations.md` and its English mirror.

## 4. Verification Record

| Command or gate | Result |
| --- | --- |
| `just check` | passed: format, all-feature Clippy, 1157 library, 4 feature-gating, 89 plugin integration, and 12 Standard integration tests |
| `RUST_TEST_THREADS=1 just check-matrix` | 42 feature checks and minimal/Standard/all-feature Clippy gates passed; minimal tests passed outside the restricted socket sandbox |
| bundle tests | minimal 698 library + 12 feature-gating + 46 integration; Standard 1042 library + 5 feature-gating + 87 integration + 12 Standard; all-feature 1157 + 4 + 89 + 12 passed |
| focused Phase 4 tests | explanation/determinism, semantic Plan, schemas 1/6 asset round trip, local template versions, execution cap and branch truncation delta, legacy SQLite migration, cache facts, upstream selection, fallback timing, and revision mismatch passed |
| WebUI gates | typecheck, lint, 19 files/113 tests, production build with 17 static pages passed |
| documentation | Chinese and English Docusaurus production builds passed |
| Windows GNU Standard check | passed; one pre-existing conditionally unused archive import warning |
| Linux x86_64 musl Standard release | passed; stripped static PIE SHA-256 `1583a489fc35ba5fad1e864b1b89efd6b81cb260797a28523827111686d5dc5e` |
| frozen-plan audit | empty diff for both frozen plans against `90a5dec` |
| repository hygiene | `git diff --check` passed; no credential is present in tracked artifacts |

The first sandboxed matrix attempt could not bind loopback sockets and failed 16
network tests with `Operation not permitted`. The same tests passed when rerun with
loopback permission; no code assertion was relaxed or skipped.

## 5. Local and Independent Isolation Evidence

The tracked credential-free harness is
`development/standard-mode/phase-4-isolation.py`, SHA-256
`8523e9df5cac5ed09f20931b55a0e87ddaecfbfce98ff40d1aabe914af9f042f`.
It embeds no credential, binds only `127.0.0.1`, changes no service/resolver/DHCP/
firewall/third-party state, uses exact child cleanup, and removes its exact run tree
between attempts.

Final local acceptance used the native Standard binary SHA-256
`ac60fcf0d1b240654309ee5b9057b5e9d242858c60165177e8df7a87a6a55950`.
It passed all seven groups with one live upstream request and ten bounded query
events. Result:

- `/tmp/oxidns-standard-phase4-local.ZVjNdW/result.json`
- result SHA-256 `59d92fd743bc9086e26b8f20bcdc9e8e5674f72c466359045240bab4bdbfde30`

The independently uploaded Linux run used only
`/tmp/oxidns-standard-phase4.7426Gn/` on the authorized isolation server. The final
uploaded binary and script matched their local hashes. All seven groups passed:

1. Standard bundle identity;
2. explanation, graph, semantic impact, recorder bounds, and forbidden-integration scan;
3. detached Expert copy and read-only analysis;
4. saved-template CRUD/duplicate/optimistic conflict;
5. transactional Apply and revision-pinned live query diagnosis;
6. current asset round trip plus schema-v1-to-v6 migration without write;
7. successful history metadata and read-only restore preview.

The remote run made one request to its loopback upstream, recorded ten bounded
events, passed exact cleanup, and a post-run exact-path process scan returned no
match. Retrieved result:

- `/tmp/oxidns-standard-phase4-local.ZVjNdW/remote-result.json`
- result SHA-256 `202ac5651b0f07b9382bbda533551ea41be7c1a6d47d15c2a7074f6f5247867a`

One initial remote attempt encountered the normal API connection window during
runtime reload. The harness was corrected to tolerate only that bounded connection
race; final local and remote runs both passed with the corrected script.

## 6. Frozen Final Acceptance

| Frozen outcome | Evidence | Result |
| --- | --- | --- |
| 1. Create/test upstream groups | Phase 1/2 WebUI, upstream API, compiler and live upstream tests; Phase 4 stable member explanation | Passed |
| 2. Select/create paths | Phase 1 path CRUD/reference safety and Phase 4 path-boundary explanation | Passed |
| 3. Cache/filter/dual-stack/ECS/fallback intent | Phase 1/2 compiler and live matrix; Phase 4 cache/ECS/fallback facts and timing | Passed |
| 4. Domain/QTYPE/response/client rules | Phase 2/3 typed rule compilation, priority, response reroute, and live integration | Passed |
| 5. Accurate pre-Apply plan/conflicts | exact-version Plan, semantic object impact, rule analysis, graph, capability gaps, YAML review | Passed |
| 6. Safe Apply preserving usable runtime | crash journal, reload rollback, restart recovery, concurrent/stale rejection, healthy history | Passed |
| 7. Explain final path/upstream/cache/fallback | revision-pinned bounded diagnosis plus local/remote live query evidence | Passed |
| 8. Modify/export/migrate/rollback intent | schemas 1-6 migration, export/import Plan, history and restore preview/Apply contract | Passed |
| 9. Explicit Expert boundary | detached copy, read-only analysis, no arbitrary reverse compilation or false ownership | Passed |
| 10. No OpenWrt/external control dependency | architecture boundary, generated graph scans, loopback isolation and docs | Passed |

The frozen long-term definition is therefore met. Standard Mode is a productized
entry point to native OxiDNS capabilities, not a browser-side YAML generator and
not an adapter for a third-party platform.

## 7. Bounded Limitations and Operating Notes

- Diagnosis explains recorded, instrumented facts; it does not invent an exhaustive
  counterfactual for events which were never recorded. Truncation is explicit.
- Historical records without a matching intent revision remain readable as raw
  facts but are intentionally not remapped to the current intent.
- Asset import and history restore are previews. Activation always requires the
  normal exact-version Apply review.
- Arbitrary Expert graphs are not reverse-compiled into Standard intent because the
  transformation is generally lossy. Users stay in Expert Mode for such graphs.
- Saved templates are local operational assets, not a synchronized marketplace.
- Diagnostic access inherits the management API security boundary; deployments
  should authenticate and restrict that API as they do other configuration data.

## 8. Maintenance Contract

Future Standard Mode changes must preserve stable intent identity, migration,
deterministic compilation, fixed priority, semantic cache/ECS isolation, exact-
version Apply, bounded diagnostics, and the native-only boundary. Any new public
intent field requires Rust/WebUI/schema/docs synchronization and the repository
change-impact/test strategy. The frozen product plans remain immutable historical
baseline documents; future work should create a new versioned product proposal.
