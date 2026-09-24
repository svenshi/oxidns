# AI Project Notes

This directory is reserved for project-maintained AI notes that do not need a
tool-mandated path.

Stable guidance currently maintained here:

- `architecture.md`: internal module ownership, dependency direction,
  lifecycle, and hot-path boundaries.
- `change-impact-matrix.md`: cross-surface synchronization and validation
  triggers for common change types.
- `maintenance.md`: recurring dependency, toolchain, feature, workspace, and
  documentation maintenance.
- `operations-runbook.md`: deployment preflight, health, diagnosis, upgrade,
  and rollback procedures.
- `performance.md`: hot-path, profiling, resource-safety, and performance
  engineering guidance.
- `plugin-dev.md`: plugin architecture, registration, feature-gating, testing,
  and documentation synchronization guidance.
- `release-process.md`: maintainer-facing release preparation workflow.
- `testing-strategy.md`: local validation ladder, CI parity, feature matrices,
  network tests, and DNS correctness rules.
- `webui.md`: WebUI-specific agent guidance.

Fixed-position files stay where tools discover them:

- `AGENTS.md` stays at the repository root and contains the canonical
  repository instructions.
- `CLAUDE.md` stays at the repository root for Claude discovery.

Put new AI-facing prompts and operating notes here unless a tool requires a
specific location.

## Content Policy

Keep these documents durable across releases. They should define project
contracts, decision criteria, repeatable workflows, and operational knowledge.
Do not use them to track planned features, target versions, task progress,
temporary migration steps, or one-off refactoring sequences; keep that work in
issues, milestones, pull requests, or the project roadmap.

Do not duplicate facts that already have an executable or machine-readable
source. Link to the owning project file instead:

- `Cargo.toml` for dependency, feature, and bundle membership.
- `justfile`, `.githooks/`, and `.github/workflows/` for exact validation,
  release, platform, and packaging commands.
- `src/plugin/*/mod.rs`, factory registration, and `src/build_info.rs` for the
  compiled plugin/capability inventory.
- `src/config/`, `config*.yaml`, and WebUI plugin definitions for configuration
  shape and defaults.
- Workspace `package.json` files and lockfiles for JavaScript tooling.

AI guidance may explain why those sources are organized as they are, how to
choose among their workflows, and which invariants a change must preserve. If a
project file and AI prose disagree about current state, the project file wins;
fix or remove the stale prose when it is relevant to the task.

## Scope And Precedence

`AGENTS.md` is the repository entry point. Read a topic document only when the
task touches that topic; release and operations guides are not general coding
instructions. Repository-wide constraints in `AGENTS.md` apply to every topic;
topic documents elaborate them and do not override them.

For current project state, prefer code, manifests, schemas, tests, workflows,
and runnable configuration over any prose inventory. Roadmaps, issues, release
history, and examples provide context but are not current-state authority.
