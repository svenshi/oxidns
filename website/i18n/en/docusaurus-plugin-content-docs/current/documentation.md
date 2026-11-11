---
title: Documentation Versions and Maintenance
---

# Documentation Versions and Maintenance

## What the current manual describes

The default manual on oxidns.org follows the OxiDNS repository `main` branch and is labeled `current` / `Next` inside Docusaurus. It therefore describes mainline capabilities and may include fields, plugins, or behavior not yet present in your installed release.

The site does not currently publish a frozen snapshot for every release. To reproduce a historical version, switch to the matching Git tag and read its `README.md`, `website/`, `config.yaml`, and release notes.

## Confirm deployment capabilities

Do not infer local capabilities from the website, archive name, or image tag alone. Check them in this order:

```bash
oxidns --version
oxidns build-info
oxidns check -c /path/to/config.yaml -d /working/directory
```

- `--version` identifies the release.
- `build-info` identifies the bundle, Cargo features, protocols, and compiled plugins.
- `check` confirms that this binary can read the real configuration and working-directory resources.
- [Release Notes](releases.md) record behavior changes, migration requirements, and release scope.

## Sources and maintenance ownership

| Content | Canonical source | Synchronization requirement |
| --- | --- | --- |
| Runnable default configuration | Repository-root `config.yaml` | Field, plugin, and default changes update the manual and WebUI definitions |
| Plugin types and compiled capabilities | Rust plugin registry and Cargo features | Content checks jointly enforce the overview, category catalogs, and bilingual references |
| CLI / API behavior | Current binary implementation | Interface changes update topic pages, examples, and migration notes together |
| Release history | Git tags and release notes | Completed work belongs in release notes instead of accumulating in the roadmap |
| Internal maintenance workflows | `docs/ai/` and `AGENTS.md` | Not published as end-user manual content |

Follow [Contributing](contributing.md) to report a documentation issue. Include the OxiDNS version, bundle, target platform, affected page, and a reproducible configuration excerpt, with credentials, private domains, and client data removed.
