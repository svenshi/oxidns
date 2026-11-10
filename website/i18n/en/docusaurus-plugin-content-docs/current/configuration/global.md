---
title: Global Configuration
---

This page covers YAML loading rules shared by every plugin and the OxiDNS top-level configuration. See the [Plugin Reference](../plugin-reference/overview.md) for plugin-specific fields.

## Before Starting

OxiDNS uses YAML configuration. For day-to-day editing, it is easiest to understand the file as six top-level parts:

```yaml
runtime:
  worker_threads: 4

api:
  http: "127.0.0.1:9088"

log:
  level: info
  file: ./oxidns.log

network:
  outbound:
    default: direct
    profiles:
      direct:
        resolver: system
        proxy: none

include: []

plugins:
  - tag: seq_main
    type: sequence
    args:
      - exec: "forward 1.1.1.1"
```

Where:

- `runtime`
  - Runtime parameters.
- `api`
  - Management API settings.
- `log`
  - Log output settings.
- `network`
  - Shared outbound networking settings, such as resolver and proxy choices for HTTP downloads, upgrade checks, and webhook requests.
- `include`
  - Load plugin definitions from other configuration files.
- `plugins`
  - All plugin instance definitions. OxiDNS composes the full DNS pipeline from plugins.

After editing a config, validate it before starting:

```bash
oxidns check -c config.yaml
```

If the config uses relative paths and the runtime working directory is not the config directory, pass the working directory explicitly. `-d` is the single base for all runtime relative paths, including logs, SQLite files, rule files, and `api.http.webui.root`; paths do not become relative to `/etc/oxidns` just because the config file lives there:

```bash
oxidns check -c /etc/oxidns/config.yaml -d /var/lib/oxidns
```

In the Debian default layout, the config file lives at `/etc/oxidns/config.yaml`, while runtime-relative resources live under `/var/lib/oxidns`.

When the plugin composition is still undecided, start from [Common Scenarios](../scenarios.md), then return to this page for field details.

## Plugin tag rules

`plugins[].tag` is the globally unique machine identifier for a plugin instance and is used directly in management API paths: `/api/plugins/{tag}/...`. A tag must meet all of these rules:

- It is 1 to 64 ASCII characters long.
- It uses only ASCII letters, digits, `_`, `-`, and `.`.
- A `.` may only separate non-empty name segments; every segment starts and ends with a letter or digit.
- `qs.exec.`, `qs.match.`, and `qs.cron.` are reserved Quick Setup prefixes and cannot be used in user configuration.

For example, `cache_main`, `cache.cn`, and `prod.cache-01` are valid. `.`, `..`, `cache..cn`, `_cache`, `cache-`, `国内缓存`, and `cache/main` are invalid.

Before upgrading from an older version, run `oxidns check -c config.yaml` with the new binary. When renaming a historical tag, update every `$tag`, `jump/goto`, plugin dependency, and other reference to it. OxiDNS does not rename tags automatically because that could silently change request-processing behavior.

## Environment Variable Substitution

During startup, `oxidns check`, management API validation, and validation before saving a config, OxiDNS first **parses the YAML into a data structure** and then expands `${VAR}` placeholders inside string scalars. The `config.yaml` file itself is not rewritten, so the WebUI still reads and saves the original placeholders.

Supported syntax:

| Syntax | Behavior |
| --- | --- |
| `${VAR}` | Use the value of process environment variable `VAR`; fail if it is undefined |
| `${VAR:-default}` | Use `default` when `VAR` is undefined or an empty string |
| `${env:VAR}` | Explicitly read process environment variable `VAR`; useful when the name conflicts with a runtime placeholder |
| `${env:VAR:-default}` | Explicitly read process environment variable `VAR`; use `default` when it is undefined or empty |
| `$${...}` | Emit a literal `${...}` |

Runtime placeholders used by executors such as `script` and `http_request` are preserved until request execution, so values like `${qname}`, `${client_ip}`, and `${resp_ip}` are not treated as process environment variables during config loading. Use the explicit form, such as `${env:qname}`, if you really need to read an environment variable with the same name.

Undefined variables fail fast, and the error includes the variable name and the YAML path of the offending scalar (for example `plugins[0].args.password`) so empty passwords or certificate paths do not silently pass validation.

Example:

```yaml
api:
  http:
    listen: ${API_LISTEN:-0.0.0.0:8080}
    ssl:
      cert: ${API_TLS_CERT}
      key: ${API_TLS_KEY}
    auth:
      type: basic
      username: ${ADMIN_USER}
      password: ${ADMIN_PASS}
```

Because substitution happens after YAML parsing, an environment value may contain any characters — `*`, `&`, `:`, `#`, `'`, `"`, `\`, newlines, even binary bytes — without breaking the config syntax. You do not need to manually quote values that contain special characters. When the entire scalar is exactly one placeholder (e.g. `timeout: ${CACHE_TTL}`), the expanded value is re-parsed once against the YAML 1.2 scalar rules, so number / boolean / `null`-shaped environment values still match numeric / boolean / null fields; everywhere else the value lands as a plain string. `include` paths support placeholders too:

```yaml
include:
  - ${OXIDNS_CONF_DIR}/plugins/common.yaml
```

## Top-Level Fields

### `include`

```yaml
# []string, load plugin settings from other configuration files.
include:
  - ./plugins/common.yaml
  - ./plugins/server.yaml
```

Field notes:

- `include`
  - Loads only `plugins` from included files. It does not merge included `runtime`, `api`, or `log` settings.
  - Merge order is include-first: recursively load each `include` in array order, then append the current file's `plugins`.
  - Relative paths are resolved from the directory of the configuration file that declares the `include`.
  - Includes may recurse up to 8 levels.
  - All merged plugin `tag` values must still be globally unique.

### `runtime`

```yaml
runtime:
  worker_threads: 4
```

Field notes:

- `worker_threads`
  - Meaning: Number of Tokio multi-thread runtime workers.
  - Default: Uses system available parallelism when omitted.
  - Constraint: Must not be `0`.

### `log`

Log timestamps use the system local timezone detected at startup, consistently across stdout, files, and the API log buffer. On OpenWrt without `/etc/localtime`, OxiDNS reads POSIX timezone rules from `/etc/TZ`; an explicit `TZ` environment variable takes precedence. Restart OxiDNS after changing the system timezone to update log timestamps.

```yaml
log:
  level: info
  file: ./oxidns.log
  rotation:
    type: daily
    max_files: 7
```

Field notes:

- `level`
  - Allowed values: `off` `trace` `debug` `info` `warn` `error`
  - Default: `info`
- `file`
  - Meaning: Optional log file path.
  - If omitted, logs go only to stdout.
  - When configured, OxiDNS writes to both stdout and the log file.
  - Log files are written as UTF-8 plain text without terminal ANSI color escape codes.
- `rotation`
  - Meaning: Log file rotation policy.
  - Default: `never`

`rotation` supports the following forms:

- `type: never`
- `type: minutely`
  - Rotate every minute.
- `type: hourly`
  - Rotate every hour.
- `type: daily`
  - Rotate every day.
- `type: weekly`
  - Rotate every week.
  - Optional `max_files` controls how many rotated files are retained; `0` disables automatic cleanup.

### `network`

`network.outbound` centralizes outbound policy for internal HTTP clients and upstreams. When omitted, behavior stays compatible: HTTP clients use system DNS with direct connections, and upstreams keep their own settings.

```yaml
network:
  outbound:
    default: direct
    profiles:
      direct:
        resolver: system
        proxy: none
      remote:
        resolver:
          nameservers:
            - addr: "1.1.1.1:53"
            - addr: "tls://dns.google:853"
              dial_addr: 8.8.8.8
            - addr: "https://cloudflare-dns.com/dns-query"
              dial_addr: 1.1.1.1
          ip_version: 4
          timeout: 5s
          proxy: none
        proxy:
          socks5: 127.0.0.1:1080
```

Field notes:

- `outbound.default`
  - Meaning: Which profile HTTP clients and upstreams use when they do not set `outbound` explicitly.
  - Default: none; without a default profile, OxiDNS uses system DNS + direct connections.
  - Constraint: If set, it must reference an existing entry in `profiles`.
  - Note: The default profile proxy is applied strictly to upstreams. Startup fails if a default SOCKS5 proxy is applied to UDP, DoQ, or DoH3 upstreams, because those connection models do not support profile proxying.
- `outbound.profiles.<name>.resolver`
  - `system`: Use system DNS. HTTP clients perform this lookup asynchronously so it does not block runtime worker threads.
  - `nameservers`: Resolve target names through configured DNS nameservers. Supports `udp://`, `tcp://`, `tls://`, `https://`, `doh://`, `h3://`, `quic://`, and `doq://`; no scheme defaults to UDP.
  - Protocol features: UDP/TCP are always available. DoT requires `resolver-dot`, DoH requires `resolver-doh`, DoQ requires `resolver-doq`, and DoH3 requires `resolver-doh3`. Legacy `upstream-*` features still enable the shared DNS client dependencies for existing build scripts, but new `network.outbound.resolver.nameservers` configs should enable `resolver-*` explicitly.
  - `ip_version`: Optional, `4` queries A records and `6` queries AAAA records. When omitted, IPv4 is used.
  - `timeout`: Optional resolver query timeout. Defaults to `5s`.
  - `proxy`: Optional. `none` connects nameservers directly; `profile` lets TCP/DoT/DoH nameservers reuse this profile's SOCKS5 proxy. UDP/DoQ/DoH3 nameservers do not support SOCKS5.
  - Domain-based nameservers must set `dial_addr`; the hostname in `addr` is kept for SNI/certificate validation and `dial_addr` is used for the actual connection.
- `outbound.profiles.<name>.proxy`
  - `none` or `direct`: Connect directly.
  - `socks5`: Connect through a SOCKS5 proxy. The format is the same as upstream `socks5`.

`download`, `upgrade`, and `http_request` can reference a profile with `args.outbound: remote`. The legacy `socks5` field remains supported. When both `outbound` and `socks5` are set on the same plugin, `socks5` overrides the profile proxy while the resolver still comes from the outbound profile. `forward` upstreams use `network.outbound.default` when `outbound` is omitted; they can also set `outbound: remote` to select another profile. Local upstream `dial_addr`, `bootstrap`, and `socks5` fields override profile-injected values.

### `api`

`api.http` supports two forms.

Shorthand:

```yaml
api:
  http: "127.0.0.1:9088"
```

Expanded form:

```yaml
api:
  http:
    listen: "127.0.0.1:9443"
    ssl:
      cert: "/etc/oxidns/api.crt"
      key: "/etc/oxidns/api.key"
      client_ca: "/etc/oxidns/client-ca.crt"
      require_client_cert: true
    auth:
      type: basic
      username: "admin"
      password: "secret"
    webui:
      root: "/etc/oxidns/webui"
      index: "index.html"
```

Field notes:

- `http.listen`
  - API listen address. Supports `ip:port`, `[ipv6]:port`, and `:port`.
  - `:port` binds as dual-stack `[::]:port`; use `0.0.0.0:port` for IPv4-only.
- `http.ssl.cert`
  - API certificate file.
- `http.ssl.key`
  - API private key file.
- `http.ssl.client_ca`
  - Optional client certificate CA.
- `http.ssl.require_client_cert`
  - Whether mutual TLS is required.
- `http.auth`
  - Currently supports `basic`.
  - See the Management API chapter for the Basic Auth header encoding rules.
- `http.cors.allowed_origins`
  - Optional WebUI/API cross-origin allowlist; when omitted, it is inferred from `http.listen`.
  - `0.0.0.0` and `[::]` automatically allow any origin; a specific IP automatically allows any WebUI port on the same host.
  - When configured explicitly, entries are matched exactly against the browser's `Origin`.
  - Use `"*"` to allow any origin, but not for credentialed browser requests.
- `http.webui.root`
  - Optional WebUI static file directory. When enabled, the WebUI is mounted at `/` and the management API is available under `/api/*`.
  - Relative paths resolve against `-d/--working-dir`; with the Debian service default `-d /var/lib/oxidns`, `root: "./webui"` means `/var/lib/oxidns/webui`.
  - See [WebUI Deployment](../webui.md) for build steps, publish directories, and standalone nginx deployment.
- `http.webui.index`
  - Optional index file name. Defaults to `index.html`.

Validation rules:

- `listen` must not be empty.
- `cert` and `key` must be configured together.
- `require_client_cert: true` requires `client_ca`.
- `basic.username` and `basic.password` must both be non-empty.
- `webui.root` must not be empty.
- `webui.index`, when configured, must not be empty.

### `plugins`

Each plugin definition uses the same outer structure:

```yaml
- tag: cache_main
  type: cache
  args:
    size: 4096
```

General rules:

- `tag`
  - Unique plugin instance identifier.
  - Must not be empty.
  - Must be unique across the whole config.
- `type`
  - Plugin type name.
  - Must match a registered plugin factory.
- `args`
  - Plugin parameters.
  - Different plugins accept different shapes: object, string, array, or null.
