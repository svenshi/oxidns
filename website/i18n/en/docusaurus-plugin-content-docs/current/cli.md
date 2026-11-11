---
title: Command-Line Tools
sidebar_position: 3
---

OxiDNS ships as a single `oxidns` binary. This page maps tasks to commands; complete options and behavior are split across three topic guides.

## Common tasks

| Goal | Command | Reference |
| --- | --- | --- |
| Validate configuration | `oxidns check -c config.yaml` | [Configuration and Data Tools](cli/tools.md) |
| Run in the foreground | `oxidns start -c config.yaml` | [Runtime, Probes, and Services](cli/runtime.md) |
| Temporarily enable debug logging | `oxidns start -c config.yaml -l debug` | [Runtime, Probes, and Services](cli/runtime.md) |
| Probe an upstream | `oxidns probe upstream tcp://1.1.1.1:53` | [Runtime, Probes, and Services](cli/runtime.md) |
| Install a system service | `sudo oxidns service install -d /var/lib/oxidns -c /etc/oxidns/config.yaml` | [Runtime, Probes, and Services](cli/runtime.md) |
| Inspect compiled capabilities | `oxidns build-info` | [Configuration and Data Tools](cli/tools.md) |
| Export dat rules | `oxidns export-dat ...` | [Configuration and Data Tools](cli/tools.md) |
| Check or apply an upgrade | `oxidns upgrade check` / `oxidns upgrade apply` | [Upgrade Command](cli/upgrade.md) |

## Help and exit status

Use `oxidns --help` for top-level commands and `oxidns <subcommand> --help` for the complete options supported by the current binary. Automation should check process status: success returns `0`; argument, validation, and runtime failures return a non-zero value.

```bash
oxidns --help
oxidns check --help
oxidns probe upstream --help
oxidns upgrade --help
```

<span id="start"></span><span id="check"></span><span id="probe"></span><span id="build-info"></span><span id="export-dat"></span><span id="service"></span><span id="upgrade"></span>

Older section bookmarks land on this entry page; use the task map above to open the extracted command guide.
