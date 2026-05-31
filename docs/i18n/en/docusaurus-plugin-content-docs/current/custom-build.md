---
title: Custom Build
sidebar_position: 6
---

# Custom Build (Cargo Features)

OxiDNS exposes optional protocols, optional plugins, and their external
dependencies as independent Cargo features. Fork the repository and either
change the `default = [...]` entry in `Cargo.toml`, or pass `--features`
on the command line, to produce a binary tailored to your scenario.

> By default `cargo build` enables the `full` bundle and produces a binary
> identical to the published release. You only enter "slimmed" mode when
> you explicitly pass `--no-default-features`.

## Three preset bundles

| Bundle | Use case | Roughly contains |
|---|---|---|
| `minimal` | Embedded / container / experimentation | UDP + TCP listeners, UDP + TCP upstreams, basic executors (sequence / forward / cache / fallback / hosts / redirect / arbitrary / dual_selector / ecs_handler / ttl / drop_resp / black_hole / debug_print / reload), all matchers, `domain_set` + `ip_set` providers. **No** hyper / rustls / quinn — smallest binary |
| `standard` | Home router / mid-range | minimal + management API + metrics + DoT/DoH/DoQ ingress & upstream + `provider-protobuf` (geoip / geosite / v2ray_dat) + adguard_rule + cron + script + download + http_request + reverse_lookup |
| `full` (default) | Everything | standard + WebUI + DoH3 ingress & upstream + MikroTik integration + query_recorder + ipset / nftset + the `upgrade` subcommand |

> Measured release binary sizes (macOS arm64, for reference): `minimal`
> ≈ 8.9 MB, `standard` ≈ 17 MB, `full` ≈ 21 MB. `minimal` excludes hyper /
> rustls / quinn / h2 / h3 / sqlite entirely, landing at roughly **40%** of
> the `full` size.

## Granular toggles

Each feature below is independently switchable. The bundle features are
just collections of these — you can also pick individual toggles and skip
the presets entirely.

### Inbound / outbound protocols

| Feature | Effect |
|---|---|
| `server-dot` | Enable DoT (TLS over TCP) inbound server, requires the rustls server stack |
| `server-doh` | Enable DoH (HTTP/2 over TLS) inbound server, requires hyper server + rustls |
| `server-doq` | Enable DoQ (QUIC) inbound server, requires `quinn` |
| `server-doh3` | Enable the HTTP/3 leg of the DoH server (needs `server-doh`), adds `h3` / `h3-quinn` / `quinn` |
| `upstream-dot` | Enable DoT upstreams (`tls://` scheme), requires the rustls client stack |
| `upstream-doh` | Enable DoH (HTTP/2) upstreams (`https://` scheme), requires hyper-rustls + `h2` |
| `upstream-doq` | Enable DoQ upstreams (`quic://` / `doq://` schemes) |
| `upstream-doh3` | Enable HTTP/3 DoH upstreams (`h3://` scheme or `enable_http3: true`, needs `upstream-doh`) |

> When a protocol is off, configs that still reference its scheme/fields
> fail at startup with a clear message, e.g. `upstream DoT is not compiled
> into this build; rebuild with --features upstream-dot`, instead of
> crashing. With `server-dot` off, putting `cert` / `key` on a
> `tcp_server` yields `DoT is not compiled into this build; rebuild with
> --features server-dot`.

### Management plane

| Feature | Effect | Dependency |
|---|---|---|
| `api` | Management / health / control / logs / config HTTP API, plus each plugin's `/plugins/<tag>/...` endpoints | hyper server + rustls server (for HTTPS) |
| `webui` | Serve the WebUI static assets from the API hub (requires `api`) | — |
| `metrics` | `/metrics` Prometheus endpoint + the `metrics_collector` executor (requires `api`) | — |

> With `api` off, the whole `src/api/` module is dropped and the hyper /
> rustls server stack goes with it — this is the main reason `minimal`
> shrinks so much. The in-process `MetricSource` counters always stay in
> core, so turning off `metrics` only removes the HTTP surface and never
> touches the hot path. `AppController` / `LogBuffer` now live in
> `src/core/`, so the core runtime (reload, shutdown, the log ring buffer)
> still works in a `minimal` build that has no `api`.

### Optional plugins

| Feature | Plugin | Main dependency |
|---|---|---|
| `plugin-mikrotik` | `ros_address_list` | `mikrotik-rs` |
| `plugin-query-recorder` | `query_recorder` | `rusqlite` (bundled SQLite) |
| `plugin-ipset` | `ipset` + `nftset` | `ripset` (Linux only) |
| `plugin-cron` | `cron` | `cronexpr` |
| `plugin-script` | `script` | — |
| `plugin-download` | `download` | — |
| `plugin-http-request` | `http_request` | — |
| `plugin-reverse-lookup` | `reverse_lookup` | — |
| `plugin-upgrade` | `upgrade` CLI subcommand + `upgrade` executor | `flate2` / `tar` / `zip` (Windows) / `semver` |
| `provider-protobuf` | `geoip` + `geosite` + `v2ray_dat` (share `prost`) | `prost` |
| `provider-adguard-rule` | `adguard_rule` | — |

## Common build commands

```bash
# Default full build (== published release)
cargo build --release

# Smallest build: bare forwarder only
cargo build --release --no-default-features --features minimal

# Home-router build (API + DoT/DoH/DoQ + common geo/adguard providers + executors)
cargo build --release --no-default-features --features standard

# Minimal plus only the MikroTik integration
cargo build --release --no-default-features --features "minimal,plugin-mikrotik"

# Bare forwarder plus the management API, nothing heavy
cargo build --release --no-default-features --features "minimal,api"
```

## Verifying the feature matrix

The repo ships `just` recipes that exercise all three bundles plus the
default-features test suite in one go:

```bash
just check-matrix
```

Or run them individually:

```bash
just check-minimal   # cargo +nightly clippy --no-default-features --features minimal
just check-standard
just check-full      # cargo +nightly clippy --all-features
```

## Runtime behavior for missing plugins

When a feature is off, the matching `#[plugin_factory("...")]` registration
block is not compiled, so the plugin type name never enters the global
factory table. A config that references a plugin not compiled into the
binary is rejected at startup by `analyze_configuration`:

```
Error: Plugin("Unknown plugin type: query_recorder")
```

This is the intended behavior — the user sees a clean error instead of a
mid-run crash.

## Common patterns after forking

1. Change `default = ["standard"]` (or any custom combination) in
   `Cargo.toml` so that `cargo build` and `cargo install` both produce the
   tailored binary out of the box.
2. If you want automatic updates against your own fork, override the
   defaults of the `upgrade` subcommand (`--repository`, `--asset`) so
   `oxidns upgrade` looks at your release feed.
