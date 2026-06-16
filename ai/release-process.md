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

## 4. Prepare GitHub Release Notes

Prepare the Markdown body that will be pasted into the GitHub Release after the
tag workflow publishes artifacts. Keep it shorter than the full documentation
release notes, but make it complete enough for operators deciding whether to
upgrade. After versions, docs, GitHub Release text, and validation are all
complete, provide the final Chinese Markdown body to the maintainer for review.

Use this standard Chinese template. A small number of emoji is allowed when it
improves scanability:

```markdown
# OxiDNS v1.3.0

## 🚀 发布概览

- 用一到两句话说明本次发布的定位、版本影响和最重要的变化。
- 说明适合升级的人群或主要收益。

## ✨ 主要亮点

- 重要功能、行为变化或兼容性改进。
- 关键 bug 修复、稳定性增强或运维体验改善。
- 如适用，补充 WebUI、打包、文档或平台相关变化。

## ⚠️ 升级说明

- 现有配置是否可以直接升级。
- 如有迁移步骤，在这里明确列出。
- 如有服务管理、WebUI、平台或配置兼容性风险，在这里说明。

## 📦 下载与校验

- 根据平台和 bundle 选择对应 archive。
- 替换生产环境二进制前，请使用 release assets 中的校验信息确认文件完整性。
```

Generation rules:

- Base the GitHub Release text on the same latest-tag-to-HEAD evidence from
  step 1 and the docs release notes from step 3.
- Do not include items that were not shipped in the tagged commit.
- Keep `Validation` limited to commands actually run for this release.
- Write the final GitHub Release body in Chinese.
- Mention breaking changes or config migrations in both `发布概览` and
  `升级说明`.
- Do not paste the full website release card verbatim; GitHub Release text
  should be concise and action-oriented.

## 5. Validate Before Tagging

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

## 6. Hand Off For Commit And Tag

Do not automatically commit, tag, or push as part of release preparation.
After versions, docs release notes, GitHub Release text, and validation are
complete, hand the final state to the maintainer with:

- A concise summary of the release-prep changes.
- The validation commands that were actually run.
- The final Chinese GitHub Release Markdown body.
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

The GitHub release workflow is triggered by pushing tags matching `v*`.
The maintainer should only push the tag after reviewing the release-prep commit
and GitHub Release text.
