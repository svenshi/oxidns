# Release Process

This file documents the repository-local workflow to follow when preparing an
OxiDNS release. It is maintainer-facing guidance, not end-user documentation.

The executable release contract is the active set of release, Docker, and
custom-build workflows under `.github/workflows/`, together with Cargo
manifests, packaging files, the upgrade asset selector, and release tests. This
guide defines review order and invariants; it does not duplicate workflow job,
target, artifact, or publication-command inventories. Changing a contract in
only one maintained surface is a release regression.

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

For each changed publishable workspace member declared by the root
`Cargo.toml`, decide whether its own manifest version or published dependency
metadata must change. Use the latest-tag path diff and current workspace
membership rather than a crate list copied into this guide.

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

## 4. Prepare GitHub Release Notes

Update `.github/release-notes.md` for the intended tag. This fixed file is
overwritten during every release preparation, while Git history retains the
previous versions. The reviewed release description therefore becomes part of
the tagged source instead of a manual post-publication edit.

Derive the required heading/version validation, delivery destinations,
rendering rules, and length handling from `.github/workflows/release.yml` plus
the notification scripts and tests it invokes. Use the current
`.github/release-notes.md` and documentation release card as structural
references instead of maintaining another template here.

Keep the curated text shorter than the full documentation release notes but
complete enough for an upgrade decision. Cover release scope, important
changes, compatibility/migration risk, and download verification.

Generation rules:

- Base the GitHub Release text on the same latest-tag-to-HEAD evidence from
  step 1 and the docs release notes from step 3.
- Do not include items that were not shipped in the tagged commit.
- Keep `Validation` limited to commands actually run for this release.
- Follow the language and section structure established by the current release
  note file and validated by the workflow.
- Make breaking changes or config migrations prominent in both the summary and
  compatibility guidance.
- Do not paste the full website release card verbatim; GitHub Release text
  should be concise and action-oriented.

Do not include a hand-written `What's Changed`, contributor list, or full
changelog link in this file; GitHub appends those generated sections during the
workflow.

## 5. Confirm The Release Artifact Contract

Read the current build matrices, bundle inputs, archive assembly, and asset
names directly from `.github/workflows/release.yml` and reusable packaging
workflows. Cross-check them against `Cargo.toml` bundle definitions,
`config*.yaml`, the upgrade asset selector and its tests, Docker download
patterns, and user installation/custom-build documentation.

The stable invariant is that every published archive name and content policy
must be understood consistently by release production, self-upgrade, Docker,
documentation, and smoke tests. Platform and target membership is deliberately
not listed here.

### Downstream publication

Derive publication order, registries, image sources, notification behavior,
required secrets, and reusable workflow inputs from the active workflow jobs.
Keep downstream consumers aligned with the release workflow; do not retain a
second job inventory in this guide.

Before tagging, compare any workflow, feature, target, packaging, or upgrade
changes against this contract. If the contract intentionally changes, update
the workflows, upgrade selection tests, install/custom-build docs, and this
section together.

## 6. Validate Before Tagging

Run the repository gate before creating the release tag:

```bash
just check
```

Run the full feature matrix when the release includes Cargo feature, optional
dependency, bundle, protocol, or broad cfg changes:

```bash
just check-matrix
```

When WebUI or docs changed, run the applicable scripts declared in their
`package.json` files and required by their active CI workflows.

Also verify before tagging:

- `Cargo.toml` package version equals the intended `vX.Y.Z` tag without the
  leading `v`.
- `Cargo.lock` contains the intended root and changed workspace crate versions.
- `.github/release-notes.md` starts with the matching version heading and
  contains only the curated notes. Keep it concise enough to avoid unnecessary
  truncation in the Telegram announcement.
- `oxidns build-info` reports the expected bundle/features for any locally
  built release candidate.
- `oxidns check` accepts the packaged full and minimal example configs under
  their corresponding bundles.
- The local package dry-run uses the same verification policy and dependency
  assumptions as the publish job in `.github/workflows/release.yml`. Any
  verification bypass must be visible in the workflow, justified by the current
  manifests, and called out as release risk; do not encode temporary dependency
  state here.
- No release-note claim depends on uncommitted working-tree changes.

## 7. Hand Off For Commit And Tag

Do not automatically commit, tag, or push as part of release preparation.
After versions, docs release notes, GitHub Release text, and validation are
complete, hand the final state to the maintainer with:

- A concise summary of the release-prep changes.
- The validation commands that were actually run.
- The reviewed `.github/release-notes.md` content.
- Suggested manual commit and tag commands.

Suggested commit message:

```text
chore(release): prepare v1.0.2
```

Suggested tag command after the maintainer has reviewed and committed the
release-prep changes:

```bash
git tag vX.Y.Z
```

The active release workflow defines its tag/ref trigger. The maintainer should
only push a matching tag after reviewing the release-prep commit and versioned
release-notes file.

Before pushing, verify the tag points at the reviewed release commit:

```bash
git show --no-patch --decorate vX.Y.Z
```

## 8. Verify Publication

Do not consider the release complete when the tag is pushed. Wait for every
required job and downstream publication declared by the tag-triggered workflows
to finish successfully.

Inspect the release asset inventory and download at least one representative
archive selected from that inventory:

```bash
gh release view vX.Y.Z --json assets
release_tmp="$(mktemp -d)"
gh release download vX.Y.Z --pattern '<asset-name-from-release-workflow>' --dir "$release_tmp"
```

Verify that:

- Every asset expected by the release workflow matrix exists with the generated
  name.
- The archive contains the expected config, license, and WebUI policy for its
  bundle.
- The extracted binary reports the intended version and build bundle.
- The example config validates with that binary.
- GitHub reports a digest for downloadable assets; the self-upgrade path relies
  on the release asset digest for SHA256 verification.
- Every downstream registry/publication job exposes the expected version and
  target metadata.
- Published package metadata agrees with the repository and tag.
- Release notes, documentation release cards, and workflow-generated
  notifications agree wherever those outputs are enabled by the workflow.

Keep a short publication record with the tag commit, workflow URL, validation
commands, and any platform not manually smoke-tested.

## 9. Failed Release And Rollback

- If local validation fails before tagging, fix the cause on a new commit and
  tag only after the reviewed commit is ready.
- If a pushed-tag workflow fails before publication and a source change is
  required, do not move the tag automatically. Explicitly decide whether to
  withdraw an unpublished tag or advance to a patch release, and record the
  decision.
- If a transient job fails for an already pushed tag but no source change is
  needed, rerun the workflow for the same tag/commit. Do not move the tag to a
  different commit silently.
- If published artifacts are incomplete, avoid announcing the release until
  the exact-tag workflow has completed and the artifact contract is verified.
- If the shipped product is defective, publish a patch release. Do not replace
  versioned assets with different binaries under the same tag.
- crates.io versions cannot be overwritten. Yank only when distribution is
  actively harmful, explain why, and follow with a corrected version.
- For bad Docker publication, preserve version-tag immutability; publish a
  corrected patch and repair moving aliases such as `latest` only with an
  explicit incident note.
- Deployment rollback follows `ai/operations-runbook.md`: restore the previous
  binary, WebUI, and config, then repeat health and DNS verification.

After any release incident, record whether the prevention belongs in CI,
packaging smoke tests, the artifact contract, or the pre-tag checklist.
