---
title: Configuration Overview
sidebar_position: 2
---

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

When the plugin composition is still undecided, start from [Common Scenarios](scenarios.md), then return to this page for field details.

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

`network.outbound` centralizes outbound policy for internal HTTP clients. When omitted, behavior stays compatible: system DNS resolution and direct connections.

```yaml
network:
  outbound:
    default: direct
    profiles:
      direct:
        resolver: system
        proxy: none
      oversea:
        resolver:
          bootstrap:
            - 1.1.1.1:53
            - 8.8.8.8:53
          bootstrap_version: 4
        proxy:
          socks5: 127.0.0.1:1080
```

Field notes:

- `outbound.default`
  - Meaning: Which profile HTTP clients use when they do not set `outbound` explicitly.
  - Default: none; without a default profile, OxiDNS uses system DNS + direct connections.
  - Constraint: If set, it must reference an existing entry in `profiles`.
- `outbound.profiles.<name>.resolver`
  - `system`: Use system DNS. HTTP clients perform this lookup asynchronously so it does not block runtime worker threads.
  - `bootstrap`: Resolve HTTP target names through the configured DNS bootstrap servers. This is useful when system DNS points back to OxiDNS itself but downloads or upgrades still need external resolution.
  - `bootstrap_version`: Optional, `4` queries A records and `6` queries AAAA records. When omitted, IPv4 is used.
- `outbound.profiles.<name>.proxy`
  - `none` or `direct`: Connect directly.
  - `socks5`: Connect through a SOCKS5 proxy. The format is the same as upstream `socks5`.

`download`, `upgrade`, and `http_request` can now reference a profile with `args.outbound: oversea`. The legacy `socks5` field remains supported. When both `outbound` and `socks5` are set on the same plugin, `socks5` overrides the profile proxy while the resolver still comes from the outbound profile.

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
  - See [WebUI Deployment](webui.md) for build steps, publish directories, and standalone nginx deployment.
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

## Responsibilities of the Four Plugin Categories

### `server`

Purpose: Accept DNS requests and send them into an executor entry.

Traits:

- Does not implement complex policy logic.
- Usually configures a bind address, TLS parameters, and an entry executor.

### `executor`

Purpose: Perform actions.

Typical actions include:

- Query upstreams
- Generate local answers
- Read and write cache
- Adjust TTL
- Handle ECS
- Run fallback and concurrent races
- Perform observability and system integrations

### `matcher`

Purpose: Evaluate conditions for use in `sequence` rules.

Typical match dimensions include:

- Query name
- Query type
- Client IP
- Response IP
- Response code
- Environment variables
- Sampling outcome
- Rate-limit state

### `provider`

Purpose: Provide reusable datasets for matchers or other plugins.

Current main provider types:

- `domain_set`
- `ip_set`
- `geoip`
- `geosite`
- `adguard_rule`

## The `sequence` Orchestration Model

`sequence` is the policy hub of OxiDNS. Most non-trivial configs use it as the primary entry.

Example:

```yaml
- tag: seq_main
  type: sequence
  args:
    - matches:
        - "$lan_clients"
        - "qtype A,28"
      exec: "$cache_main"
    - matches: "!has_resp"
      exec: "$forward_main"
    - exec: "accept"
```

Each rule has two key fields:

- `matches`
  - One matcher expression or an array of expressions.
  - When it is an array, every condition must be true for the rule to match.
- `exec`
  - The action to execute when the rule matches.

## Referencing Plugins and Quick Setup

### Reference Existing Plugins

Use `$tag` to reference a plugin that has already been defined:

```yaml
- exec: "$forward_main"
- matches:
    - "$is_internal"
    - "!has_resp"
  exec: "$cache_main"
```

### Quick Setup

If a `sequence` rule uses `type + arguments` instead of `$tag`, OxiDNS creates a temporary plugin on the fly.

Example:

```yaml
- exec: "forward 1.1.1.1 8.8.8.8"
- matches: "qname domain:example.com"
  exec: "ttl 300"
```

Common quick setup forms today:

- matcher
  - `_true`
  - `_false`
  - `qname ...`
  - `qtype ...`
  - `qclass ...`
  - `client_ip ...`
  - `resp_ip ...`
  - `ptr_ip ...`
  - `cname ...`
  - `mark ...`
  - `env ...`
  - `random ...`
  - `rate_limiter ...`
  - `rcode ...`
  - `has_resp`
  - `has_wanted_ans`
  - `string_exp ...`
- executor
  - `forward ...`
  - `cache ...`
  - `ttl ...`
  - `prefer_ipv4`
  - `prefer_ipv6`
  - `sleep ...`
  - `debug_print ...`
  - `query_summary ...`
  - `metrics_collector ...`
  - `black_hole ...`
  - `drop_resp`
  - `ecs_handler ...`
  - `forward_edns0opt ...`
  - `ipset ...`
  - `nftset ...`
  - `upgrade ...`
  - `download ...`
  - `reload_provider ...`
  - `reload`

## Built-In `sequence` Control Flow

Besides calling plugins, `sequence.args[].exec` can also use built-in control flow:

### `accept`

- Ends the current `sequence` immediately.
- This is an explicit early stop, so callers do not continue with later rules.
- Does not build a response by itself.
- Typical use:
  - Close out the pipeline after `cache`, `hosts`, or `arbitrary` has already written a response.
  - Stop later `forward` or side-effect stages once a branch has already made the decision.

### `return`

- Ends the current `sequence` immediately and returns control to the caller.
- Does not build a response.
- If the current `sequence` was entered via `jump`, the caller resumes at the rule after `jump`.
- If the current `sequence` is the top-level entry, this acts like an early exit from the current rule chain.

### `reject [rcode]`

- Builds a DNS response from the current request immediately and ends the current `sequence`.
- The default `rcode` is `REFUSED`, so plain `reject` means “reject this request”.
- A decimal numeric code or English RCODE name can be provided explicitly; English names are case-insensitive. Common mappings and meanings are listed in the [DNS Code Reference](dns-codes.md#rcode-response-codes), for example:
  - `reject 2` => `SERVFAIL`
  - `reject SERVFAIL` / `reject servfail` => `SERVFAIL`
  - `reject 3` => `NXDOMAIN`
  - `reject NXDOMAIN` => `NXDOMAIN`
- `reject` only supports base DNS RCODEs `0..15`; extended RCODEs require an EDNS OPT and are not generated by this built-in action.
- `reject 0` returns a plain `NOERROR` response and does not add an SOA automatically.
- Callers do not continue with later rules.
- A typical use is returning a specific error code directly, for example:

```yaml
- matches: "qtype HTTPS"
  exec: "reject NXDOMAIN"
```

### `mark ...`

- Inserts one or more unsigned integer marks into `DnsContext.marks`.
- Supported forms:
  - `mark 1`
  - `mark 1 2 3`
  - `mark 1,2,3`
- Continues to the next rule in the current `sequence`.
- Does not build a response and does not terminate the current `sequence`.

### `jump seq_tag`

- Calls another `sequence`; conceptually this behaves like a subroutine call.
- The parameter must be the target `sequence` tag without a leading `$`.
- If the called `sequence`:
  - reaches its tail normally, the current `sequence` resumes at the rule after `jump`.
  - executes `return`, the current `sequence` also resumes at the rule after `jump`.
  - executes `accept`, `reject`, or another operation that returns `Stop`, the current `sequence` stops as well.

### `goto seq_tag`

- Transfers control to another `sequence`; conceptually this behaves like a one-way jump.
- The parameter must be the target `sequence` tag without a leading `$`.
- The current `sequence` never resumes after `goto`:
  - If the target `sequence` reaches its tail, control does not return to the rules after `goto`.
  - If the target `sequence` executes `return`, that `return` is propagated outward and still does not return to the rules after `goto`.
  - If the target `sequence` executes `accept`, `reject`, or another `Stop`, that result propagates outward directly.
- This is useful when ownership of the request should be handed off permanently to another policy branch.

Example:

```yaml
- matches: "$rate_ok"
  exec: "mark 100"
- matches: "!$rate_ok"
  exec: "reject 2"
```

Example showing the difference between `jump` and `goto`:

```yaml
- tag: child_seq
  type: sequence
  args:
    - exec: "mark 2"
    - exec: "return"

- tag: parent_jump
  type: sequence
  args:
    - exec: "mark 1"
    - exec: "jump child_seq"
    - exec: "mark 3"

- tag: parent_goto
  type: sequence
  args:
    - exec: "mark 1"
    - exec: "goto child_seq"
    - exec: "mark 3"
```

- `parent_jump` ends with marks `1,2,3` because execution resumes after `jump`.
- `parent_goto` ends with marks `1,2` because execution never returns after `goto`.

## Common Rule Syntax

### Domain Rules

These forms appear in plugins such as `qname`, `cname`, `domain_set`, `hosts`, and `redirect`:

- `full:example.com`
  - Exact match.
- `domain:example.com`
  - Suffix match.
- `keyword:cdn`
  - Substring match.
- `regexp:^api[0-9]+\\.example\\.com$`
  - Regular-expression match.
- `example.com`
  - Without a prefix, common domain-rule users such as `qname`, `cname`, and
    `domain_set` usually treat it as `domain:example.com`; `hosts` and
    `redirect` treat it as an exact `full:example.com` match.

### IP Rules

These forms appear in `client_ip`, `resp_ip`, `ptr_ip`, `ip_set`, and related plugins:

- Single IP: `1.1.1.1`
- CIDR: `192.168.0.0/16`
- IPv6 CIDR: `2400:3200::/32`

### Provider References

Matchers and providers can reference providers through:

- `$tag`
  - References a defined provider with the required match capability.
  - Domain-oriented references can target `domain_set` or `geosite`.
  - IP-oriented references can target `ip_set` or `geoip`.
- `&/path/to/file`
  - Loads rules directly from a file.

Example:

```yaml
args:
  - "domain:example.com"
  - "$core_domains"
  - "&/etc/oxidns/domains.txt"
```

## Unified Upstream Structure

`forward.upstreams` uses a shared `UpstreamConfig` shape.

Example:

```yaml
upstreams:
  - addr: "udp://1.1.1.1:53"
  - addr: "https://resolver.example/dns-query"
    bootstrap: "8.8.8.8:53"
    timeout: 5s
    enable_http3: true
```

Common fields:

- `addr`
  - Upstream address.
  - Defaults to UDP when no scheme is given.
  - Supports `udp://`, `tcp://`, `tcp+pipeline://`, `tls://`, `tls+pipeline://`, `quic://`, `doq://`, `https://`, `doh://`, and `h3://`.
  - DoH should include the full path, for example `https://resolver.example/dns-query`.
- `dial_addr`
  - Actual connection IP, while keeping the hostname from `addr` for SNI and certificate validation.
- `port`
  - Overrides the port.
- `bootstrap`
  - Bootstrap DNS used to resolve the upstream hostname when `addr` is domain-based. Must be `IP:port`.
- `bootstrap_version`
  - `4` or `6`.
- `socks5`
  - SOCKS5 proxy.
  - Supports `host:port` and `user:pass@host:port`.
  - IPv6 must use `[addr]:port`.
- `idle_timeout`
  - Idle connection timeout in seconds.
- `min_conns`
  - Minimum warmed pool connections. Default: `0`; range: `0..4096`; must not exceed `max_conns`.
- `max_conns`
  - Maximum pool size, in the range `1..4096`.
- `insecure_skip_verify`
  - Skips TLS certificate validation. Recommended only for test environments.
- `timeout`
  - Per-query timeout. Default: `5s`.
- `enable_pipeline`
  - Enables TCP or DoT pipelining.
- `enable_http3`
  - Uses HTTP/3 for DoH.
- `so_mark`
  - Linux `SO_MARK`.
- `bind_to_device`
  - Linux `SO_BINDTODEVICE`.
