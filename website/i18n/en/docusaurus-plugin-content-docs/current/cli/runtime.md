---
title: Runtime, Probes, and Services
---


This page covers foreground runtime, upstream probing, and operating-system service management. Use the same working directory as the production service for all relative runtime paths.

## `start`

Starts OxiDNS in the foreground.

Typical usage:

```bash
oxidns start -c config.yaml
oxidns start -c config.yaml -l debug
oxidns start -c /etc/oxidns/config.yaml -d /var/lib/oxidns
```

Arguments:

- `-c, --config <PATH>`
  - Path to the configuration file.
  - Default: `config.yaml`
- `-d, --working-dir <PATH>`
  - Change to the specified working directory before startup.
  - All runtime relative paths use this directory as their base, including logs, SQLite files, rule files, and `api.http.webui.root`.
  - In the Debian default layout, the config lives at `/etc/oxidns/config.yaml`, while runtime-relative resources live under `/var/lib/oxidns`.
- `-l, --log-level <LEVEL>`
  - Temporarily override the configured log level.
  - Supported values: `off` `trace` `debug` `info` `warn` `error`

Common use cases:

- Local debugging
- Foreground execution
- Direct container startup

## `probe`

Actively probes runtime-facing external targets. The current subcommand is `probe upstream`, which checks one DNS upstream for reachability, basic response details, hostname resolution, and concurrency / pipeline behavior.

### `probe upstream`

Typical usage:

```bash
oxidns probe upstream udp://1.1.1.1:53
oxidns probe upstream tcp://1.1.1.1:53
oxidns probe upstream tls://dns.google:853 --qname example.com. --qtype A
oxidns probe upstream https://dns.google/dns-query --json
oxidns probe upstream tcp://dns.example.com:53 -c config.yaml --outbound remote
```

Arguments:

- `<addr>`
  - Upstream address to probe.
  - Accepts the same address forms as forward upstreams, including `udp://`, `tcp://`, `tcp+pipeline://`, `tls://`, `tls+pipeline://`, `https://`, `doh://`, `h3://`, `quic://`, and `doq://`.
  - Addresses without a scheme are treated as UDP.
- `-c, --config <PATH>`
  - Optionally read a configuration file and reuse only its `network.outbound` profiles.
  - When omitted, no runtime config is read.
- `-d, --working-dir <PATH>`
  - Change the working directory before reading the config.
- `--outbound <NAME>`
  - Use resolver / proxy settings from the named outbound profile.
- `--dial-addr <IP>`
  - Connect directly to the specified IP while preserving the hostname from `<addr>` for TLS SNI and HTTP Host.
- `--bootstrap <ADDR>`
  - Use the specified bootstrap DNS server to resolve hostname upstreams.
- `--bootstrap-version <4|6>`
  - Preferred IP version for bootstrap resolution.
- `--socks5 <ADDR>`
  - Use a SOCKS5 proxy for upstream transports that support proxying.
- `--port <PORT>`
  - Override the upstream port.
- `--insecure-skip-verify`
  - Skip TLS certificate verification. Use only for testing.
- `--timeout <DURATION>`
  - Per-query timeout.
  - Default: `5s`
- `--qname <NAME>`
  - Query name used for the serial baseline.
  - Default: `example.com.`
- `--qtype <TYPE>`
  - Query type.
  - Default: `A`
- `--serial-samples <N>`
  - Number of serial baseline queries.
  - Default: `2`
- `--pipeline-concurrency <N>`
  - Number of concurrent probe queries. For TCP / DoT, these queries are forced onto one connection.
  - Default: `16`
- `--pipeline-rounds <N>`
  - Number of concurrency probe rounds.
  - Default: `2`
- `--json`
  - Print a structured JSON report.

Output includes:

- Target details: address, protocol, server name, port, and timeout.
- Hostname upstream resolution: `resolved_ip` and `resolution_source`; sources may be `literal`, `dial_addr`, `configured`, `bootstrap`, `system`, or `proxy`.
- Serial baseline: reachable / unreachable, average latency, rcode, answer count, TC / RA flags, and error summary.
- Concurrency probe: supported / unsupported / unstable / inconclusive, success count, timeout count, response ID / question / qtype mismatch count, other errors, and recommendation.
- Non-JSON mode prints probe progress to stderr while the final report goes to stdout. JSON mode writes only the report to stdout.

Protocol behavior:

- UDP, DoH, DoH3, and DoQ use the matching upstream implementation to send concurrent queries and evaluate concurrency or multiplexing behavior for that protocol.
- TCP and DoT additionally force concurrent queries through one connection to detect pipeline-specific timeouts, connection closes, protocol errors, response ID confusion, or crossed questions.
- If the serial baseline fails, the concurrency verdict is `inconclusive` so a basic reachability problem is not misclassified as a pipeline problem.

## `service`

Manages system service installation and runtime state.

Supported subcommands:

- `service install`
- `service start`
- `service stop`
- `service restart`
- `service uninstall`

### `service install`

Installs the service definition without starting it immediately.

```bash
sudo oxidns service install -d /var/lib/oxidns -c /etc/oxidns/config.yaml
```

Arguments:

- `-d, --working-dir <PATH>`
  - Service working directory, and the base for all runtime relative paths inside the service.
  - Must be an absolute path.
  - The generated service passes this to OxiDNS through `ExecStart ... -d <PATH>`; if a custom systemd unit also sets `WorkingDirectory=`, keep both values aligned.
- `-c, --config <PATH>`
  - Configuration path used by the installed service.

### `service start`

Starts the installed system service.

```bash
sudo oxidns service start
```

### `service stop`

Stops the installed system service.

```bash
sudo oxidns service stop
```

### `service restart`

Restarts the installed system service.

```bash
sudo oxidns service restart
```

### `service uninstall`

Removes the installed system service.

```bash
sudo oxidns service uninstall
```
