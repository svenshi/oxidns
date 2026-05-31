![OxiDNS Banner](.github/img/logo-banner.png)

[![oxidns downloads](https://img.shields.io/github/downloads/SvenShi/oxidns/total)](https://github.com/SvenShi/oxidns/releases)
[![Rust CI](https://github.com/svenshi/oxidns/actions/workflows/rust-ci.yml/badge.svg?branch=main)](https://github.com/svenshi/oxidns/actions/workflows/rust-ci.yml)
[![WebUI CI](https://github.com/svenshi/oxidns/actions/workflows/webui-ci.yml/badge.svg)](https://github.com/svenshi/oxidns/actions/workflows/webui-ci.yml)

[中文](README.md) | [English](README_EN.md) · [Documentation](https://oxidns.org/en/) · [Quick Start](https://oxidns.org/en/quickstart) · [Plugin Reference](https://oxidns.org/en/plugin-reference/overview)

# OxiDNS

**A high-performance DNS policy orchestration engine for complex networks.**

OxiDNS is a modern DNS engine built with Rust. It is inspired by [mosdns](https://github.com/IrineSistiana/mosdns), but it is not merely another rule-based DNS forwarder.

It focuses on the full lifecycle of DNS queries in real-world network environments: ingress, matching, caching, forwarding, fallback, rewriting, local answers, and system integrations, with built-in query recording, Prometheus metrics collection, and real-time logging.

The core idea of OxiDNS is not to expose more switches. It is to provide a clear, composable, and debuggable policy pipeline that lets you describe complex DNS behavior through declarative configuration.

```text
server -> DnsContext -> matcher / executor / provider -> upstream
```

The project is under active development. It is designed for users who need fine-grained control over DNS behavior and are willing to understand its policy model.

---

## Why OxiDNS

In complex networks, DNS is often more than “resolve this domain”.

You may need to:

- Select different upstreams based on domain, client, query type, response IP, or response code
- Apply different policies to different devices, subnets, or scenarios
- Race, fallback, fail over, or make decisions based on upstream results
- Adjust TTL, handle ECS, rewrite responses, or return local answers
- Sync DNS results into `ipset`, `nftset`, or MikroTik RouterOS
- Record query behavior and understand system state through logs, query records, and Prometheus plugin metrics
- Reload configuration, rules, and providers without interrupting the service

OxiDNS provides a unified orchestration model for these scenarios instead of a collection of isolated feature switches.

---

## Design Principles

### Composable

OxiDNS decomposes DNS processing into `matcher`, `executor`, `provider`, and `sequence`.

Each component has a focused responsibility, and complete policies are built by composing them into pipelines.

### Debuggable

Once DNS policies become complex, the most important question is not just “does it run”, but “why did it behave this way”.

OxiDNS provides query recording (`query_recorder`), query summary statistics (`query_summary`), Prometheus plugin metrics (`metrics_collector`), real-time structured logging, and configuration validation. Users can clearly understand which matchers were evaluated, which executors ran, which upstream was selected, and why a fallback path was taken for any given query.

### Evolvable

OxiDNS is designed for long-running self-hosted network environments.

It supports full hot reload, provider-scoped hot reload, separately built WebUI hosting, and keeps room for future plugin and operations-oriented improvements.

### Explicit

OxiDNS does not try to hide complexity from you.

It is better suited for users who want explicit control over DNS behavior, rather than users who only want a one-click DNS dashboard.

---

## Core Capabilities

| Category | Capabilities |
| --- | --- |
| Protocols | UDP, TCP, DoT, DoQ, DoH |
| Policy model | `sequence`, `matcher`, `executor`, `provider` |
| Executors | `forward`, `cache`, `fallback`, `hosts`, `arbitrary`, `redirect`, `ecs_handler`, `ttl`, `ip_selector`, `download`, `upgrade`, `reload`, `reload_provider`, `script`, `http_request`, `learn_domain`, `query_summary`, `query_recorder`, `metrics_collector` |
| Matchers | `qname`, `question`, `qtype`, `qclass`, `client_ip`, `resp_ip`, `rcode`, `rate_limiter`, and more |
| Data sets | `domain_set`, `dynamic_domain_set`, `ip_set`, `geoip`, `geosite`, `adguard_rule` |
| System integrations | `ipset`, `nftset`, `ros_address_list`, `reverse_lookup` |
| Debugging and operations | Health checks, config validation, hot reload, query records, Prometheus plugin metrics, real-time logs |
| Deployment | Multi-platform builds, Debian packages, standalone WebUI hosting, service installation |

---

## Good Fits

OxiDNS is a good fit for DNS environments that need to be long-running, debuggable, and extensible.

Typical use cases include:

- Home gateways, side routers, OpenWrt, NAS, and homelab setups
- Multi-upstream racing, fallback chains, and mixed protocol environments
- Fine-grained DNS policy routing based on domains, clients, and response results
- DNS-result-driven `ipset` / `nftset` / MikroTik address list synchronization
- Ad filtering, domain routing, local overrides, dual-stack preferences, and ECS control
- Self-hosted DNS infrastructure that needs explicit control and debugging
- Lightweight deployments that serve a separately built WebUI on the same management port

---

## Non-Goals

OxiDNS is not a one-click DNS dashboard for everyone.

If you primarily need:

- Simple and ready-to-use home ad blocking
- A full graphical DNS management experience
- Authoritative DNS hosting
- A Kubernetes service discovery plugin framework
- A zero-configuration tool that does not require understanding its configuration model

Then AdGuard Home, Pi-hole, Technitium DNS Server, or CoreDNS may be a better fit.

OxiDNS is for users who want to describe DNS behavior explicitly through configuration and are willing to accept some complexity in exchange for control.

---

## Relationship to Other Projects

OxiDNS does not try to replace every DNS tool:

| Project | Best suited for |
| --- | --- |
| AdGuard Home | Ready-to-use home ad blocking and DNS management |
| Pi-hole | Simple, mature, community-proven home DNS filtering |
| CoreDNS | Cloud-native DNS and service discovery plugin framework |
| Technitium DNS Server | Full-featured general-purpose DNS server |
| mosdns | Flexible DNS routing and policy processing |
| OxiDNS | High-performance, debuggable, extensible DNS policy orchestration |

---

## Download

Install the latest release with one command. By default this installs and starts OxiDNS as a system service:

```bash
curl -fsSL https://oxidns.org/install.sh | sudo sh
```

Elevated Windows PowerShell:

```powershell
irm https://oxidns.org/install.ps1 | iex
```

By default, Linux / macOS installs into `/opt/oxidns`, creates `/usr/local/bin/oxidns`, and installs and starts the system service. Windows installs into `%ProgramFiles%\OxiDNS`, adds it to the Machine PATH, and installs and starts the service. For a portable user install, set `OXIDNS_INSTALL_SERVICE=0`; see Quick Start for details.

Uninstall while keeping `config.yaml`:

```bash
curl -fsSL https://oxidns.org/uninstall.sh | sudo sh
```

Elevated Windows PowerShell:

```powershell
irm https://oxidns.org/uninstall.ps1 | iex
```

If you installed with `sudo` or a custom `OXIDNS_INSTALL_DIR`, use the same privilege level and directory variable when uninstalling.

If you want to download a GitHub release directly, use this platform guide:

| System / Environment | Recommended release asset |
| --- | --- |
| Linux x86_64 | `oxidns-x86_64-unknown-linux-musl.tar.gz` |
| Linux ARM64 | `oxidns-aarch64-unknown-linux-musl.tar.gz` |
| Debian / Ubuntu x86_64 service install | `*_amd64.deb` |
| Debian / Ubuntu ARM64 service install | `*_arm64.deb` |
| Alpine Linux x86_64 | `oxidns-x86_64-unknown-linux-musl.tar.gz` |
| Alpine Linux ARM64 | `oxidns-aarch64-unknown-linux-musl.tar.gz` |
| 32-bit ARM Linux, including some Raspberry Pi installs | `oxidns-arm-unknown-linux-musleabihf.tar.gz` |
| macOS Intel | `oxidns-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `oxidns-aarch64-apple-darwin.tar.gz` |
| Windows x64 | `oxidns-x86_64-pc-windows-msvc.zip` |
| Windows 32-bit | `oxidns-i686-pc-windows-msvc.zip` |
| Windows ARM64 | `oxidns-aarch64-pc-windows-msvc.zip` |
| FreeBSD x86_64 | `oxidns-x86_64-unknown-freebsd.tar.gz` |

On Linux, prefer the `musl` build if you are unsure about compatibility.

If you are unsure which platform you are on, run:

```bash
uname -s && uname -m
```

On Windows PowerShell, run:

```powershell
(Get-CimInstance Win32_OperatingSystem).OSArchitecture
```

For the full installation flow, see [Quick Start](https://oxidns.org/en/quickstart).

### Slim builds

OxiDNS lets you strip optional protocols and plugins via Cargo features. When building from source:

```bash
cargo build --release                                                  # default = full
cargo build --release --no-default-features --features minimal         # bare forwarder
cargo build --release --no-default-features --features standard        # home / router
```

See [Custom Build](https://oxidns.org/en/custom-build) for details.

---

## Documentation

- [Configuration](https://oxidns.org/en/configuration)
- [Quick Start](https://oxidns.org/en/quickstart)
- [Plugin Overview](https://oxidns.org/en/plugin-reference/overview)
- [Management API](https://oxidns.org/en/api)
- [MikroTik Policy Routing](https://oxidns.org/en/mikrotik-policy-routing)
- [Common Scenarios](https://oxidns.org/en/scenarios)
- [Architecture and Design](https://oxidns.org/en/architecture-and-design)
- [Performance and Benchmarks](https://oxidns.org/en/benchmarks)
- [Roadmap](https://oxidns.org/en/roadmap)

---

## Roadmap

The following outlines the planned development directions in delivery order. See the [documentation roadmap](https://oxidns.org/en/roadmap) for full details.

1. **Custom builds**: Split compilation by plugin module so users can fork, select only the plugins they need, and auto-update from a custom repository
2. **IP optimization**: Probe multiple A/AAAA addresses from a DNS response in parallel and return the lowest-latency IP to the client
3. **MikroTik deep integration**: Add the ability to pull RouterOS address lists as a data source and to actively push local IP sets to RouterOS
4. **OpenWrt support**: One-command install via opkg with automatic service management — a native deployment experience for OpenWrt users
5. **WebUI and metrics improvements**: Add management interfaces for new plugins and expand Prometheus metric coverage

Looking further ahead, two plugin extension mechanisms are planned: WebAssembly plugins and dynamic library plugins, enabling third-party developers to build and distribute plugins independently.

---

## Project Status

OxiDNS is under active development.

The current version is suitable for advanced users, testing environments, and self-hosted network setups. For production use, make sure you understand the configuration, logs, and fallback behavior before deploying it.

Issues, real-world feedback, documentation improvements, and plugin contributions are welcome.

---

## Disclaimer

This project is provided as-is, without warranties of any kind.

DNS infrastructure directly affects network availability, name resolution results, and access behavior. Misconfiguration can cause connectivity loss, DNS leaks, or unexpected resolution failures. Before deploying in production or critical environments, make sure you understand the configuration model, have tested fallback paths, and have monitoring in place.

The maintainers are not responsible for any service disruption, data loss, or security incident resulting from the use of this software. Users are responsible for ensuring their deployment and usage comply with applicable laws, regulations, and third-party service terms.

---

## Community

Join the Telegram group to chat with the author and other users: [**@OXIDNS** · https://t.me/oxidns](https://t.me/oxidns)

<a href="https://t.me/oxidns">
  <img src=".github/img/telegram-qr.png" alt="OxiDNS Telegram group QR code" width="220" />
</a>

---

## License

This project is licensed under the [GNU General Public License v3.0 or later](LICENSE).
