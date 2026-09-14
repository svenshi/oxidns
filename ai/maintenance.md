# Maintenance Guide

This document defines recurring repository maintenance outside feature work and
release preparation. Release-specific versioning and publication remain in
`ai/release-process.md`; vulnerability handling remains in `SECURITY.md`.

## Maintenance Goals

- Keep supported bundles and platforms buildable.
- Keep dependency, feature, docs, WebUI, and packaging representations aligned.
- Reduce obsolete code and unsafe compatibility assumptions deliberately.
- Preserve reproducible builds and operational rollback paths.
- Prevent routine upgrades from becoming large mixed-risk changes.

## Toolchain Contract

- Rust edition and dependency requirements come from workspace Cargo manifests.
- Formatting options come from `rustfmt.toml`; local toolchain invocations come
  from `justfile` and `.githooks/`; CI and release toolchains come from the
  active workflows under `.github/workflows/`.
- JavaScript runtime and package-manager versions come from the WebUI/docs
  manifests, lockfiles, and their CI workflows.
- Committed lockfiles are reproducibility artifacts and must be updated through
  the package manager that owns the corresponding manifest.

Do not change toolchain versions in only one workflow. Check local guidance,
all CI workflows, installer/build documentation, and reusable custom builds.

## Dependency Update Policy

Dependabot opens weekly grouped updates for:

- Cargo dependencies.
- GitHub Actions.
- Docker dependencies.

WebUI and docs JavaScript dependencies require explicit maintenance because
they are not covered by Dependabot.

For each dependency update:

1. Read upstream release notes for behavior, feature, MSRV, platform, security,
   and default-feature changes.
2. Identify every direct and transitive usage. Use `cargo tree -i <crate>` for
   Rust dependencies when ownership is unclear.
3. Keep unrelated major upgrades in separate commits/PRs.
4. Preserve optional dependency gating; a new optional crate must not leak into
   `minimal` unless intentionally required.
5. Update the manifest and lockfile through the native package manager.
6. Run affected focused tests, then the validation required below.
7. Call out generated-code, wire-format, TLS, database, or persistence changes.

Derive renamed packages, forks, patches, and source overrides from the current
Cargo manifests and lockfile. Treat changes to any such dependency as source
changes: review the exact release/source diff and run the owning subsystem's
tests. Do not preserve a temporary dependency explanation in this guide after
the manifest no longer expresses it.

## Validation by Dependency Type

### Rust patch/minor updates

```bash
just check
```

Use the feature-matrix recipe currently declared in `justfile` for optional
dependencies, feature graph changes, proc-macros, async runtime/networking,
TLS/HTTP/QUIC, serialization, or platform integration updates.

### Rust major updates

- Update one subsystem at a time.
- Inspect public API and behavior migrations rather than relying only on a
  successful compile.
- Run the full feature matrix and review affected request paths and resource
  bounds for hot-path libraries.
- Let Linux, Windows, and macOS CI complete before merge.

### WebUI dependencies

Use the package manager and scripts declared by `webui/package.json` and its
lockfile. Match CI coverage in `.github/workflows/webui-ci.yml`. Review framework
and build-output changes for runtime and static export impact.

### Docs dependencies

Use the package manager and scripts declared by `docs/package.json` and its
lockfile. Match CI coverage in `.github/workflows/docs-ci.yml`; use the
lockfile-preserving install mode selected there when validating reproducibility.

### GitHub Actions and Docker

- Review permission changes and action input/output changes.
- Keep release and reusable custom-build target/packaging logic aligned.
- For Docker base/runtime changes, build locally without push and run the image
  smoke checks used by the workflow.

## Feature and Bundle Hygiene

Whenever features change, select the per-feature, bundle, and powerset recipes
currently declared in `justfile`. The active schedule and CI depth are defined
by `.github/workflows/rust-ci.yml`.

Check that:

- Public features follow category naming rules.
- Private `_` aggregators are not documented as user-facing switches.
- Optional dependencies are reachable only from intended features.
- Bundle membership in `Cargo.toml` matches `src/build_info.rs`, custom-build
  documentation, feature-gating tests, and release packaging workflows.
- Disabled-feature fallback paths remain warning-free.

When the active workflow runs unused-dependency analysis, review suggested
removals manually: proc-macro, build-script, platform-only, and feature-only
dependencies may not look used in the active configuration.

## Workspace Crate Maintenance

The workspace members declared in the root `Cargo.toml` are authoritative.

- Keep each crate's manifest metadata and version internally consistent.
- Update root path dependency version requirements when a child crate version
  changes.
- Do not bump every crate for every OxiDNS release; bump a crate when its code
  or published dependency contract changed.
- Run workspace tests/docs after shared protocol or proc-macro changes.
- Verify `cargo publish --dry-run` for any crate intended for publication.

The root release workflow publishes the root crate. Publishing child crates or
changing their publication order requires an explicit release workflow change.

## Code Health

Recurring cleanup should look for:

- Modules that combine unrelated config, model, lifecycle, metrics,
  persistence, and protocol responsibilities.
- Duplicate parsing or transport abstractions.
- `infra -> plugin` dependency regressions.
- Unbounded queues, maps, retry loops, or background tasks.
- Blocking work or high-cardinality logging/metrics in request paths.
- Deprecated config aliases whose removal window has passed.
- Platform cfg branches not exercised by local development.
- Tests that depend on fixed ports, public networks, sleeps, or global state.
- Stale examples and mismatched Chinese/English documentation.

Structural cleanup must preserve behavior unless the PR explicitly declares a
behavior change. Use `ai/architecture.md` for placement decisions.

## Configuration and Persistence Evolution

- Prefer additive optional fields with defaults that preserve old behavior.
- Reject unknown fields where silent typos would be dangerous.
- Keep error messages tied to config paths and plugin tags.
- Document renamed/removed fields and provide an upgrade path.
- Version or detect persistence formats when incompatible evolution is
  possible.
- Test old fixtures, corrupt/truncated data, and partial write recovery.
- Do not remove compatibility code solely because current tests generate only
  the newest format.

Use `ai/change-impact-matrix.md` to synchronize WebUI schemas, examples, API
payloads, user docs, and release notes.

## Documentation Maintenance

Quarterly or before a significant release, check:

- `AGENTS.md` matches the actual top-level and package structure.
- Every file listed in `ai/README.md` exists and remains authoritative.
- Plugin types, config fields, metrics, and examples match between Rust,
  Chinese docs, English docs, WebUI definitions, and locales.
- Commands match the current Clap subcommand structure.
- Release and operations documents match workflow and service behavior.
- Historical release notes remain historical; do not rewrite old claims merely
  because paths or current behavior changed later.

## Suggested Cadence

### Weekly

- Triage Dependabot and security advisories.
- Review CI failures, nightly feature powerset, and flaky-test evidence.
- Check release/operations issues for recurring failure patterns.

### Monthly

- Update WebUI/docs dependencies in bounded batches.
- Review outdated direct dependencies and patched Git sources.
- Run or inspect bundle/feature matrix results.
- Review warnings, deprecations, and platform compatibility notices.

### Before a minor or major release

- Audit config/API/persistence compatibility.
- Review bundle contents and release target matrix.
- Re-run relevant performance baselines for hot-path changes.
- Validate upgrade and rollback documentation.
- Follow the complete `ai/release-process.md` workflow.

## Maintenance Handoff

Maintenance PRs should state:

- Why the update is needed now.
- Direct and transitive scope.
- Feature, platform, config, persistence, and performance risk.
- Lockfiles and generated artifacts changed.
- Exact commands run.
- Any CI/platform result still required before merge.
