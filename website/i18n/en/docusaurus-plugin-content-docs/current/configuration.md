---
title: Configuration Overview
sidebar_position: 2
---

OxiDNS uses YAML to describe runtime settings, the management interface, shared networking, and plugin execution chains. Use this page to choose the right configuration topic. The repository-root `config.yaml` remains the canonical runnable example.

## Shortest configuration workflow

1. Start from the default `config.yaml` or [Common Scenarios](scenarios.md); do not assemble a non-trivial plugin graph from an empty file.
2. Use [Global Configuration](configuration/global.md) for workers, logging, network egress, the API, and plugin instances.
3. Use [Execution Chains and Control Flow](configuration/sequence.md) to compose servers, executors, matchers, and providers.
4. For domains, IPs, and external datasets, follow [Rules and Provider Syntax](configuration/rules.md).
5. Validate every change before reloading or restarting:

```bash
oxidns check -c config.yaml
```

If configuration and runtime data use different directories, validate with the real working directory. The default Debian layout uses:

```bash
oxidns check -c /etc/oxidns/config.yaml -d /var/lib/oxidns
```

## Topic map

| Topic | Contents |
| --- | --- |
| [Global Configuration](configuration/global.md) | Top-level fields, `include`, environment variables, tags, runtime, logging, networking, API, and plugins |
| [Execution Chains and Control Flow](configuration/sequence.md) | Plugin-category responsibilities, `sequence`, plugin references, quick setup, and `accept` / `return` / `reject` / `jump` / `goto` |
| [Rules and Provider Syntax](configuration/rules.md) | Domain rules, IP/CIDR, `$tag` provider references, and external files |
| [Plugin Reference](plugin-reference/overview.md) | Complete per-plugin parameters, defaults, examples, and platform constraints |

## Source ownership

- This chapter owns the shared configuration model and does not duplicate every plugin field table.
- Plugin-specific options belong in the [Plugin Reference](plugin-reference/overview.md).
- CLI flags belong in [Command-Line Tools](cli.md), while HTTP schemas belong in the [Management API](api.mdx).
- The release `config.yaml` is the canonical runnable example for that version. Run `oxidns check` with the new binary after an upgrade.

<span id="include"></span><span id="runtime"></span><span id="log"></span><span id="network"></span><span id="api"></span><span id="plugins"></span>

If an older bookmark opened this page, use the topic map above to reach the extracted guide.
