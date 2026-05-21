# Release Process

This file documents the repository-local workflow to follow when preparing an
OxiDNS release. It is maintainer-facing guidance, not end-user documentation.

## 1. Build The Release Story From Tags

Start from the latest release tag and use the changes since that tag as the
source of truth for the release scope.

Recommended commands:

```bash
LATEST_TAG=$(git tag --list 'v*' --sort=-v:refname | head -n 1)
echo "$LATEST_TAG"
git log --oneline --decorate --no-merges "$LATEST_TAG"..HEAD
git diff --stat "$LATEST_TAG"..HEAD
git diff --name-only "$LATEST_TAG"..HEAD
```

Use the commit log and diff together:

- Summarize user-visible behavior, compatibility impact, operational changes,
  and bug fixes from `LATEST_TAG..HEAD`.
- Check touched subsystems before deciding whether the release is patch, minor,
  or major.
- Do not invent release-note items that are not visible in the commit range or
  the current diff.
- If the working tree contains release-prep edits, keep them separate in your
  reasoning from product changes since the previous tag.

## 2. Update Cargo Versions

Update the root package version for every release:

- `Cargo.toml` at the repository root, `[package].version`

If any crate under `crates/` has code changes since the latest release tag, bump
that crate's own `Cargo.toml` too:

- `crates/macros/Cargo.toml`
- `crates/proto/Cargo.toml`
- `crates/ripset/Cargo.toml`
- `crates/zoneparser/Cargo.toml`

Use path-level diffs to decide which crate versions need to change:

```bash
git diff --name-only "$LATEST_TAG"..HEAD -- crates/macros
git diff --name-only "$LATEST_TAG"..HEAD -- crates/proto
git diff --name-only "$LATEST_TAG"..HEAD -- crates/ripset
git diff --name-only "$LATEST_TAG"..HEAD -- crates/zoneparser
```

When a crate version changes:

- Update the crate's `[package].version`.
- Update any local dependency version declarations that refer to that crate,
  including root `Cargo.toml` path dependencies.
- Refresh `Cargo.lock` through a normal Cargo command such as `cargo check` or
  the release validation command.

Do not bump a workspace crate just because the root package is being released;
bump it only when that crate changed or its published dependency metadata must
change.

## 3. Generate Release Notes In Docs

Update both release-note files:

- `docs/docs/releases.md`
- `docs/i18n/en/docusaurus-plugin-content-docs/current/releases.md`

Follow the existing `ReleaseCard` format. For a new latest release:

- Insert the new card at the top of the matching month section, or add a new
  `## YYYY-MM` section if needed.
- Set the card version to the release tag, for example `v1.0.2`.
- Choose the badge from the semantic version impact, such as `Patch Release`,
  `Minor Release`, or `Major Release`.
- Use the intended release date in `YYYY-MM-DD` format.
- Move `defaultOpen` to the newest card only.
- Keep the Chinese file and English i18n file aligned in content and structure.

Use the established sections:

- Chinese: `版本定位`, `主要变更`, `配置与升级说明`
- English: `Release Scope`, `Changes`, `Compatibility and Upgrade Notes`

The content should be generated from the latest-tag-to-HEAD changes gathered in
step 1. The upgrade notes must mention:

- The root crate version and expected release tag.
- Whether existing configs can upgrade directly.
- Any new, renamed, or behavior-changing config fields.
- Any operational cautions, migration steps, or compatibility risks.

## 4. Validate Before Tagging

Run the relevant quality gates before creating the release tag:

```bash
cargo +nightly fmt
cargo test
```

Also run these when the corresponding areas changed:

```bash
cd webui && pnpm typecheck
cd docs && npm run build
```

Prefer `just check` for the final full gate when time allows.

## 5. Commit And Tag

Commit release-prep changes with a conventional message, for example:

```text
chore(release): prepare v1.0.2
```

Create the release tag on the release-prep commit:

```bash
git tag vX.Y.Z
```

The GitHub release workflow is triggered by pushing tags matching `v*`.
Only push the tag after versions, docs release notes, and validation are
complete.
