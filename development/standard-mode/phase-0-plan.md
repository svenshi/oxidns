# Standard Mode Phase 0 Implementation Plan

Status: complete

Frozen product baseline: `docs/docs/standard-mode-plan.md` at commit `90a5dec`

## 1. Input and Requirement Freeze

Phase 0 has no preceding development-phase handoff. The frozen product baseline at
`90a5dec` is therefore the initial handoff input for this phase. It was reviewed
before this plan was written.

The frozen Chinese and English product-plan documents are requirements sources and
must not be edited during phased implementation. Clarifications, implementation
decisions, discovered constraints, validation evidence, and deferred work belong in
this plan or the phase handoff instead of the frozen documents.

## 2. Objective

Move Standard Mode from an unsafe Alpha compiler to a trustworthy Beta compiler by
closing every Phase 0 requirement in the frozen plan:

- SM-0.1 deterministic compilation correctness;
- SM-0.2 one authoritative semantic validation result;
- SM-0.3 explicit configuration ownership and safe mode switching;
- SM-0.4 backend Plan/Apply transaction with failure recovery;
- SM-0.5 direct compiler, migration, transaction, and live DNS tests.

No Phase 1 feature work is included. In particular, this phase does not add complete
upstream-group CRUD, encrypted inbound listeners, smart domestic/remote routing,
dynamic learning, or new scenario templates.

## 3. Evidence-based Baseline

The following findings were verified against the current worktree before planning:

| Frozen requirement | Current evidence | Phase 0 action |
|---|---|---|
| Runtime cache fields | `webui/lib/standard-mode/generator.ts` emits `min_ttl`, `max_ttl`, and `negative_ttl` | Emit the real positive/negative TTL fields from the authoritative compiler |
| Cache isolation | All paths execute `standard_cache`; cache keys do not encode path or upstream group | Generate one cache plugin per effective path semantics |
| ECS cache key | ECS is stored in Standard state but not compiled | Hide/deprecate the unavailable control in this phase and reject non-inherit values until Phase 2 |
| Runtime threads | Generator emits `runtime.threads`; Rust expects `runtime.worker_threads` | Compile the canonical field and cover it with a config-analysis test |
| Path-local feature enablement | Plugin creation checks only global/device filtering and logging | Compute effective path/device policy before capability planning |
| Invalid references | Schema normalization and generator silently replace missing groups/paths with the first/default item | Preserve invalid references for validation and fail Plan/Apply |
| Upstream strategy | `parallel`, `fastest`, and `sequential` only alter `concurrent` | Replace the model with runtime-aligned response selection; reject ordered fallback until an explicit fallback model exists |
| Capability degradation | Generator accumulates `skippedCapabilities` and continues | Core requested capabilities become blocking diagnostics; only optional metrics may warn |
| Ownership | Mode can switch to Standard by patching WebUI state without inspecting the current DNS config | Add `managed`, `modified`, and `unmanaged` analysis plus explicit takeover confirmation |
| Apply transaction | Standard state, DNS YAML, metadata, and reload are committed in separate frontend calls | Move Plan/Apply authority to Rust and stage/commit/recover the state as one managed operation |
| Direct tests | No direct tests exist for `webui/lib/standard-mode/*` | Add backend compiler/API tests and focused frontend contract tests |

## 4. Target Phase 0 Architecture

### 4.1 Authority boundary

The Rust backend is the canonical authority for:

```text
StandardIntent JSON
  -> schema decoding and migration
  -> normalization
  -> semantic and capability validation
  -> PolicyPlan
  -> generated YAML / PluginGraph
  -> OxiDNS configuration analysis
  -> ownership and semantic diff
  -> transactional apply request
```

The WebUI retains form state and local convenience validation, but it must call the
backend plan endpoint before save/apply and must not independently generate the
runtime configuration.

### 4.2 Module ownership

- `src/config/standard_mode/`: intent types, normalization, validation, compiler,
  tags, policy plan, and deterministic serialization. It must remain independent of
  React/WebUI code and must not perform filesystem or runtime operations.
- `src/api/standard_mode.rs`: Plan/Apply HTTP payloads, ownership analysis, version
  checks, staging, transaction journal, and API responses.
- `src/infra/control.rs`: only generic application-control messages and status;
  Standard Mode models must not leak into `infra`.
- `src/app.rs`: execution of a staged generic configuration apply, runtime result
  reporting, and transaction finalization/rollback after the existing API listener
  restarts.
- `webui/lib/standard-mode/`: form types, API adapters, display diagnostics, and
  compatibility migration. Runtime generation is removed or reduced to a typed
  adapter for backend responses.

### 4.3 Phase 0 API contract

`POST /standard/plan` accepts:

```text
intent
base_config_version
base_standard_version
takeover: false | true
```

It returns:

```text
normalized_intent
policy_plan
generated_yaml
generated_config_version
ownership
semantic_diff
diagnostics[]
can_apply
```

`POST /standard/apply` accepts the same intent plus the exact planned config version
and both base versions. It rejects stale plans, stale DNS configuration, stale
Standard state, invalid ownership confirmation, missing required capabilities, and
an already-running apply.

Apply returns an accepted transaction identifier. The WebUI observes completion
through transaction/reload status after the API listener returns. A successful
runtime transition finalizes Standard state and generated metadata; a failed
transition restores the previous DNS file and leaves the previous Standard state as
authoritative.

### 4.4 Ownership rules

- `managed`: stored generated metadata matches both intent revision and current DNS
  configuration version.
- `modified`: a prior managed record exists but either the generated revision or DNS
  version differs.
- `unmanaged`: no complete prior generated record exists.
- Plan is always available for inspection.
- Apply requires `takeover: true` for `modified` or `unmanaged` configuration.
- The plan lists preserved top-level fields and replaced plugin tags.
- Phase 0 preserves `include`, `api`, and `network`; it owns `runtime.worker_threads`,
  `log.level`, and the complete plugin list only after explicit takeover.

## 5. Work Sequence

### Step A — Establish the compiler contract and tests

1. Add Rust `StandardIntent`, normalized intent, diagnostics, ownership, tag map,
   summary, PolicyPlan, and generation result types.
2. Add fixtures for minimal/default, multiple paths, capability failures, invalid
   references, duplicate identifiers, and migrations.
3. Add deterministic ordering/tag tests before replacing frontend generation.

Completion evidence:

- focused Rust unit tests pass;
- repeated compilation produces byte-identical YAML and metadata;
- invalid input produces stable diagnostic codes and object paths.

### Step B — Correct SM-0.1 compilation semantics

1. Emit `min_positive_ttl`, `max_positive_ttl`, `max_negative_ttl`, and
   `negative_ttl_without_soa`.
2. Generate a cache per effective Resolution Path and map each path to its cache tag.
3. Emit `runtime.worker_threads`.
4. Generate filtering and recorder plugins whenever an effective path or device
   explicitly enables them.
5. Compile `fastest`, `balanced`, `prefer_positive`, and `consensus` to
   `forward.response_selection`.
6. Remove `parallel`, `sequential`, and ambiguous fallback behavior through schema
   migration; ordered fallback remains unavailable until its explicit Phase 1/2
   model exists.
7. Treat ECS, dual-stack, IP selection, sampling, and scenario values as unsupported
   Phase 0 input unless they remain at their inert defaults; hide their controls.

Completion evidence:

- golden configuration tests assert exact plugin graphs;
- every currently exposed Standard setting has an output-difference or validation
  test;
- generated YAML passes `config::validate_text`.

### Step C — Implement unified SM-0.2 validation

1. Validate IDs, generated tags, names, subscription filenames, and listener keys
   globally before normalization can erase conflicts.
2. Validate every upstream group, path, routing rule, exception, device, and provider
   reference.
3. Validate enabled upstream availability, listener selection/address conflicts, TTL
   ranges, subscription URL/intervals, and capability requirements.
4. Return `error`, `warning`, and `suggestion` severities with stable codes.
5. Make generation impossible when any error exists.
6. Keep frontend validators as field-local hints sourced from the same diagnostic
   codes; backend validation decides `can_apply`.

Completion evidence:

- all silent first/default fallbacks are removed;
- missing core plugins fail planning;
- optional metrics absence is the only silent-runtime-safe warning class.

### Step D — Implement SM-0.3 ownership and safe switching

1. Parse existing Standard metadata defensively and classify ownership.
2. Compare current DNS version, intent revision, generated version, and managed tags.
3. Generate a semantic takeover diff with preserved/replaced/removed surfaces.
4. Change mode selection so choosing Standard Mode does not persist takeover or
   overwrite YAML.
5. Require an explicit confirmation dialog for `modified` and `unmanaged` apply.
6. Remove the Standard system page's direct reuse of Expert save paths.

Completion evidence:

- switching views alone performs no DNS-config write;
- unmanaged and modified configurations cannot apply without confirmation;
- managed, unchanged configuration applies without a destructive warning;
- missing metadata is classified as `unmanaged`, never in-sync.

### Step E — Implement SM-0.4 Plan/Apply transaction

1. Add and register `/standard/plan`, `/standard/apply`, and transaction-status API
   routes.
2. Check DNS and Standard-state base versions together under a single apply lock.
3. Stage candidate DNS YAML, Standard state, generated metadata, and a recovery
   journal beside the live configuration.
4. Ask the application loop to attempt the staged configuration without committing
   Standard state first.
5. On runtime success, atomically replace managed files in a recoverable order and
   mark the journal committed.
6. On runtime failure, restore the previous DNS runtime and files, preserve previous
   Standard state, and record bounded failure diagnostics.
7. Recover or roll back an incomplete transaction during startup before normal
   configuration loading.
8. Update the WebUI to plan, display diagnostics/ownership/diff, apply, and refresh
   authoritative state after completion.

Completion evidence:

- version conflicts are deterministic HTTP 409 responses;
- failed runtime initialization leaves old files/state and old service behavior;
- simulated failure at each file transition recovers to either the old or new complete
  transaction, never a mixed authoritative state;
- concurrent apply is rejected;
- transaction work remains outside the DNS request path.

### Step F — Complete SM-0.5 tests and quality gates

1. Rust unit tests for schema, migration, normalization, validation, tags, ownership,
   compiler features, transaction journal, and corruption recovery.
2. API handler tests for plan, apply, conflict, takeover, capability errors, busy
   transaction, status, success, and rollback.
3. Frontend tests for intent/API mapping, diagnostic rendering, mode switching, and
   takeover confirmation.
4. Plugin integration tests for generated default and multi-path configurations.
5. Live UDP/TCP tests using local mock upstreams and ephemeral ports to prove final
   path selection and cache isolation.

## 6. Acceptance Evidence Matrix

| Exit gate | Required evidence |
|---|---|
| No exposed inert setting | UI audit plus per-setting compiler/validation tests |
| Compiler modules covered | Rust unit test inventory mapped to each compiler module |
| Stable output | deterministic YAML/tag golden tests |
| Cache isolation | graph assertions plus live cross-path cache test |
| Apply failure preserves state | API/app transaction failure-injection tests |
| Unmanaged config protected | ownership unit tests and WebUI flow test |
| Full quality gate | exact command output recorded in the phase handoff |

Required commands before handoff:

```bash
pnpm typecheck
pnpm lint
pnpm test
pnpm build
cargo test
cargo test --test plugin_integration
just check
```

If API feature boundaries change, also run the applicable minimal/standard checks and
`just check-matrix` when feasible.

## 7. Compatibility and Performance Constraints

- Existing Expert YAML remains valid and is never rewritten by merely opening or
  selecting Standard Mode.
- Schema v1 and v2 Standard state remain readable through explicit migration.
- Public API additions require Chinese and English API documentation.
- Runtime/config/plugin field changes require synchronized frontend types and both
  locales.
- No compiler, filesystem, journal, or explanation work runs in the DNS request hot
  path.
- Generated paths perform no request-time rule parsing or unbounded iteration.
- Phase 0 does not add an OpenWrt, UCI, firewall, system DNS, RouterOS, ipset, nftset,
  or third-party proxy dependency.

## 8. Handoff and Commit Protocol

Before Phase 0 is declared complete:

1. Create `development/standard-mode/phase-0-handoff.md`.
2. Record every completed work package, architectural decision, changed contract,
   migration, exact test command/result, residual risk, and Phase 1 prerequisite.
3. Re-read the frozen plan and map each Phase 0 exit gate to evidence.
4. Inspect staged and unstaged changes using the `git-commit-comment` skill workflow.
5. Generate a Conventional Commit comment from the verified whole-stage diff.
6. Commit the complete Phase 0 implementation and handoff locally.
7. At the start of Phase 1, read the Phase 0 handoff before writing the Phase 1 plan.
