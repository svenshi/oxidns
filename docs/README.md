# Project Documentation

This directory contains repository-maintained project documentation.

- [Benchmarking and performance CI](benchmarks.md): measurement boundaries,
  regression checks, and automated PR feedback.
- [AI and agent guidance](ai/README.md): architecture, development, testing,
  maintenance, release, and operations workflows.

The user-facing Docusaurus documentation site lives in [`website/`](../website/).
Its build commands are defined in [`website/package.json`](../website/package.json)
and validated by [Docs CI](../.github/workflows/docs-ci.yml).

Keep project documentation here and user-facing manuals, translations, and site
assets in `website/`. Tool-discovered instructions remain at the repository root
in [`AGENTS.md`](../AGENTS.md) and [`CLAUDE.md`](../CLAUDE.md).
