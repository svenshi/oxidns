---
title: CLI Tools
sidebar_position: 3
---

This page explains the OxiDNS CLI by day-to-day task. For normal deployment, the most common flow is to run `check` first and then `start`.

OxiDNS ships a single executable: `oxidns`.

Available top-level commands:

- `start`
- `check`
- `build-info`
- `export-dat`
- `probe`
- `service`
- `upgrade`

## Common Tasks

| Goal | Command |
| --- | --- |
| Validate a config | `oxidns check -c config.yaml` |
| Start in the foreground | `oxidns start -c config.yaml` |
| Temporarily enable debug logging | `oxidns start -c config.yaml -l debug` |
| Print the plugin dependency graph | `oxidns check -c config.yaml --graph` |
| Inspect compiled binary capabilities | `oxidns build-info` |
| Probe upstream reachability and concurrency behavior | `oxidns probe upstream tcp://1.1.1.1:53` |
| Install as a system service | `sudo oxidns service install -d /var/lib/oxidns -c /etc/oxidns/config.yaml` |
| Check for a new release | `oxidns upgrade check` |
| Export rules from a dat file | `oxidns export-dat --file ./rules/geosite.dat --kind geosite --selector cn --out-dir ./rules/exported` |

## Help

Show top-level help:

```bash
oxidns --help
```

Show help for a specific subcommand:

```bash
oxidns start --help
oxidns check --help
oxidns build-info --help
oxidns export-dat --help
oxidns probe --help
oxidns probe upstream --help
oxidns service --help
oxidns upgrade --help
```

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

## `check`

Statically validates a configuration file without starting OxiDNS.

Typical usage:

```bash
oxidns check -c config.yaml
oxidns check -c /etc/oxidns/config.yaml
oxidns check -c /etc/oxidns/config.yaml -d /var/lib/oxidns
oxidns check -c config.yaml --graph
```

Arguments:

- `-c, --config <PATH>`
  - Path to the configuration file.
  - Default: `config.yaml`
- `-d, --working-dir <PATH>`
  - Change to the specified working directory before validation.
  - Useful when the config relies on relative paths.
  - Keep it the same as the runtime `-d` value so validation and startup see the same relative paths.
- `--graph`
  - Print the plugin dependency graph after validation succeeds.

Behavior:

- Performs static validation only:
  - YAML parsing
  - schema-level config validation
  - plugin type and dependency validation
- Does not initialize plugins, bind listeners, or start the runtime.
- On success, exits with code `0` and prints a short success line.
- With `--graph`, it also prints a plain-text dependency graph in plugin initialization order.
- On failure, exits non-zero and prints the validation error.

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

## `build-info`

Prints the compile-time capabilities of the current `oxidns` binary.

Typical usage:

```bash
oxidns build-info
```

Behavior:

- Does not read a configuration file, start the runtime, or bind any ports.
- Prints formatted JSON.
- The output includes:
  - `version`: current package version.
  - `bundle`: primary build bundle for this binary: `minimal`, `standard`, `full`, or `custom`.
  - `enabled_bundles`: bundle features compiled into the binary.
  - `enabled_features`: public Cargo features compiled into the binary.
  - `supported_plugins`: server, executor, matcher, and provider plugin types supported by this binary.
- The returned capability object matches the `build` field returned by the management API `GET /api/build`.

Common use cases:

- Confirm whether the installed binary is `minimal`, `standard`, `full`, or a custom build.
- Check whether a protocol, plugin, or the `upgrade` subcommand is compiled into the current binary.
- Compare capabilities before and after custom builds, package validation, or upgrades.

## `export-dat`

Exports selected rules from `geosite.dat` or `geoip.dat` into text rule files.

These exported files can be referenced directly from `domain_set.files` or `ip_set.files`.

Typical usage:

```bash
oxidns export-dat \
  --file ./rules/geosite.dat \
  --selector cn \
  --selector geolocation-\!cn \
  --out-dir ./rules/exported
```

Generate an additional merged union file:

```bash
oxidns export-dat \
  --file ./rules/geosite.dat \
  --kind geosite \
  --selector cn \
  --selector mastercard@cn \
  --out-dir ./rules/exported \
  --merged-file geosite_union.txt
```

Export from `geoip.dat`:

```bash
oxidns export-dat \
  --file ./rules/geoip.dat \
  --kind geoip \
  --selector cn \
  --out-dir ./rules/exported
```

Export the entire dat file without selectors:

```bash
oxidns export-dat \
  --file ./rules/geosite.dat \
  --kind geosite \
  --out-dir ./rules/exported
```

Export using the original text format:

```bash
oxidns export-dat \
  --file ./rules/geosite.dat \
  --kind geosite \
  --format original \
  --selector cn \
  --out-dir ./rules/exported
```

Arguments:

- `--file <PATH>`
  - Path to the source `dat` file.
- `--kind <KIND>`
  - Explicit `dat` kind.
  - Values: `auto` `geosite` `geoip`
  - Default: `auto`
- `--format <FORMAT>`
  - Output text format.
  - Values: `oxidns` `original`
  - Default: `oxidns`
- `--selector <SELECTOR>`
  - Selector to export.
  - Repeat the flag to export multiple selectors.
  - Omit it to export the entire dat file.
- `--out-dir <DIR>`
  - Output directory.
  - It is created automatically when missing.
- `--merged-file <NAME>`
  - Optional.
  - Writes one extra merged union file inside the output directory.
- `--overwrite`
  - Optional.
  - Allows replacing existing output files.

Behavior:

- By default, OxiDNS writes one file per selector, for example `cn.txt` or `geolocation-!cn.txt`.
- When no selector is provided, OxiDNS writes one full-export file named `geosite.txt` or `geoip.txt` by default.
- `geosite` exports OxiDNS domain rule expressions such as `full:`, `domain:`, `keyword:`, and `regexp:`.
- In `oxidns` format, exported files add a header comment such as `# selector: cn`; when no selector is provided, the header becomes `# selector: all`.
- In `original` format, `geosite` preserves the source type names and writes values such as `plain:`, `regex:`, `root_domain:`, and `full:`.
- In `original` format, `geosite` output is grouped by code, and domain attributes are appended after the domain text, for example `@cn` or `@ads=1`.
- `geoip` exports plain IP / CIDR lines.
- In `oxidns` format, `geoip` exports also include selector header comments.
- In `original` format, `geoip` output is grouped by code with section headers like `[code]`.
- `geosite` selectors support `code@attribute`, for example `mastercard@cn`.
- If any selector matches no rules, the command fails instead of silently skipping it.

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

## `upgrade`

Checks, downloads, or applies OxiDNS upgrades from GitHub Releases.

Supported subcommands:

- `upgrade check`
- `upgrade download`
- `upgrade apply`

Common usage:

```bash
oxidns upgrade
oxidns upgrade --force
oxidns upgrade check
oxidns upgrade download --target latest
sudo oxidns upgrade apply
sudo oxidns upgrade apply --no-restart
```

Common arguments:

- `--target <TAG|latest>`
  - Release tag or `latest`.
  - Default: `latest`
- `--repository <OWNER/REPO>`
  - GitHub repository.
  - Default: `svenshi/oxidns`
- `--asset <NAME|auto>`
  - Release asset name. `auto` selects the archive for the current platform and build bundle.
  - Default: `auto`
- `-c, --config <PATH>`
  - Runtime configuration file used to read `api.http.webui.root` when `--webui-dir` is not set.
  - When omitted, `upgrade` first checks `config.yaml` in the current directory. On Linux package installs, it also uses `/etc/oxidns/config.yaml` when present.
- `-d, --working-dir <DIR>`
  - Base directory for runtime-relative paths, with the same semantics as `start -d/--working-dir`.
  - When omitted and the Linux package configuration is detected, `/var/lib/oxidns` is used; otherwise the current directory is used.
- `--bundle <auto|full|standard|minimal>`
  - Selects the release build bundle when `--asset auto` is used.
  - Default: `auto`, which follows the current binary's build bundle.
  - `full` uses the legacy asset name, for example `oxidns-x86_64-unknown-linux-musl.tar.gz`; `standard` / `minimal` use slim asset names such as `oxidns-standard-x86_64-unknown-linux-musl.tar.gz`.
- `--cache-dir <DIR>`
  - Directory for cached upgrade files.
  - Default: `./upgrade-cache`
- `--backup-dir <DIR>`
  - Directory for binary backups before `apply`.
  - Default: `./upgrade-backups`
- `--webui-dir <DIR>`
  - Directory where the WebUI static assets are installed during `apply`; relative paths are resolved against `-d/--working-dir`, and should stay aligned with `api.http.webui.root`.
  - When omitted, `upgrade` first infers it from `api.http.webui.root`; if no WebUI root is configured, it uses `./webui`.
- `--skip-webui`
  - For `apply`, skip the WebUI directory upgrade and replace only the binary.
- `--no-restart`
  - Skip restarting the service after a successful `apply`. By default the installed service is restarted automatically via the system service manager (systemd / launchd / Windows SCM).
- `--allow-prerelease`
  - Allows prerelease releases.
- `--force`
  - For `apply`, continue downloading, verifying, and replacing even when the selected release is not newer than the current version.
- `--timeout <DURATION>`
  - HTTP timeout such as `30s` or `2m`.
- `--socks5 <ADDR>`
  - Optional SOCKS5 proxy.
- `--insecure-skip-verify`
  - Disables TLS certificate verification.
- `--github-token <TOKEN>`
  - GitHub personal access token for API requests, used to raise the rate limit or access private repositories.

Behavior:

- `check` only queries the release and compares versions.
- `download` downloads the archive and verifies SHA256 with the GitHub release asset `digest` field.
- An explicit `--asset` always wins and skips `--bundle` inference.
- Omitting the subcommand defaults to `apply`.
- `apply` updates only when a newer version is available by default. `--force` forces the update.
- On Unix, `apply` unpacks the `.tar.gz`, backs up the current binary, and replaces it. On Windows, `apply` unpacks the `.zip`, backs up and replaces the binary, and also upgrades the WebUI directory.
- By default, after replacing the binary `apply` backs up and installs the archive's `webui/` directory into `--webui-dir`; `--skip-webui` skips it, and an archive without `webui/` is skipped without affecting the binary upgrade.
- In the default Debian package layout, `sudo oxidns upgrade apply` infers the WebUI directory from `/etc/oxidns/config.yaml` and `/var/lib/oxidns`; when `/var/lib/oxidns/webui` is a symlink, the real target directory is updated.
- After a successful `apply`, the service is restarted automatically via the system service manager. Pass `--no-restart` to skip the automatic restart.
- After a successful `apply`, the CLI asks whether to clean the cache and backup directories. The default answer is `Y`.

## Page Scope

This page covers the commands above. To confirm every argument supported by the local binary, run `oxidns <subcommand> --help`.
