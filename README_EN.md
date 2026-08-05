![OxiDNS Banner](.github/img/logo-banner.png)

[![oxidns downloads](https://img.shields.io/github/downloads/SvenShi/oxidns/total)](https://github.com/SvenShi/oxidns/releases)
[![latest release](https://img.shields.io/github/v/release/svenshi/oxidns)](https://github.com/svenshi/oxidns/releases/latest)
[![license](https://img.shields.io/github/license/svenshi/oxidns)](LICENSE)
[![Rust CI](https://github.com/svenshi/oxidns/actions/workflows/rust-ci.yml/badge.svg?branch=main)](https://github.com/svenshi/oxidns/actions/workflows/rust-ci.yml)
[![WebUI CI](https://github.com/svenshi/oxidns/actions/workflows/webui-ci.yml/badge.svg)](https://github.com/svenshi/oxidns/actions/workflows/webui-ci.yml)

[中文](README.md) | [English](README_EN.md) · [Documentation](https://oxidns.org/en/) · [Quick Start](https://oxidns.org/en/quickstart) · [Plugin Reference](https://oxidns.org/en/plugin-reference/overview)

# OxiDNS

**A high-performance DNS policy orchestration engine for complex networks.**

OxiDNS is a plugin-driven DNS engine built with Rust for software routers, OpenWrt, homelabs, and advanced self-hosted networks. Declarative policies compose matching, caching, forwarding, fallback, rewriting, local answers, and system integrations, while the WebUI, management API, query records, Prometheus metrics, and real-time logs provide the control plane.

Inspired by [mosdns](https://github.com/IrineSistiana/mosdns), OxiDNS goes beyond rule-based forwarding: requests, upstream responses, and network side effects share one composable and explainable policy pipeline, so complex behavior can still be configured, validated, and traced.

[Quick Start](https://oxidns.org/en/quickstart) · [Configuration](https://oxidns.org/en/configuration) · [Common Scenarios](https://oxidns.org/en/scenarios) · [Performance and Benchmarks](https://oxidns.org/en/benchmarks)

## Why OxiDNS

- **Composable policy**: `sequence` combines matchers, executors, and providers into reusable branches, jumps, and fallback chains.
- **Explainable decisions**: Query records, execution paths, structured logs, and metrics show what matched, what ran, and why a result was selected.
- **Unified ingress and egress**: One policy serves UDP, TCP, DoT, DoQ, and DoH while upstream resolution, connection reuse, concurrent selection, and proxy egress stay centrally managed.
- **DNS-driven networking**: Resolution results can update Linux `ipset` / `nftset`, RouterOS address lists and static routes, HTTP webhooks, and external scripts.

OxiDNS has its own DNS message and wire codec, and moves rule compilation, dependency analysis, and connection initialization out of the request hot path whenever possible. See [Performance and Benchmarks](https://oxidns.org/en/benchmarks) for the methodology and historical results.

The request path stays explicit:

```text
client -> server -> sequence (matcher + executor + provider)
                         |-> upstream / local answer -> response
                         `-> side effects
```

## What a policy looks like

This configuration sends `corp.lan` to an internal resolver and everything else to an encrypted upstream. Providers, matchers, executors, and servers compose through tags instead of embedding policy in protocol listeners:

```yaml
plugins:
  - tag: internal_domains
    type: domain_set
    args:
      exps: ["domain:corp.lan"]

  - tag: forward_internal
    type: forward
    args:
      upstreams:
        - addr: "udp://192.168.1.1:53"

  - tag: forward_public
    type: forward
    args:
      upstreams:
        - addr: "tls://dns.quad9.net:853"
          bootstrap: "9.9.9.9:53"

  - tag: main
    type: sequence
    args:
      - matches: "qname $internal_domains"
        exec: "$forward_internal"
      - matches: "!has_resp"
        exec: "$forward_public"
      - exec: accept

  - tag: dns_server
    type: udp_server
    args:
      entry: main
      listen: ":5335"
```

See [Configuration](https://oxidns.org/en/configuration) and [Common Scenarios](https://oxidns.org/en/scenarios) for the complete structure, reusable rules, and deployment patterns.

---

## Core Capabilities

| Category | Capabilities |
| --- | --- |
| Protocol ingress | UDP, TCP, DoT, DoQ, DoH over HTTP/1.1, HTTP/2, and HTTP/3 |
| Policy orchestration | `sequence`, conditional matching, executor composition, jumps, and fallback chains |
| Upstreams and egress | Multi-protocol upstreams, concurrent response selection, connection reuse, bootstrap, SOCKS5, unified `network.outbound` |
| Response processing | TTL-aware positive and negative caching, ECS and ECS-derived client IPs, local records, redirects, response construction, dual-stack and IP selection |
| Rule data | Domain and IP sets, GeoIP, GeoSite, AdGuard rules, dynamic domain learning |
| System integrations | Linux `ipset` / `nftset`, RouterOS address lists / static routes, HTTP webhooks, external scripts |
| Observability | Query auditing and execution paths, real-time logs, Prometheus metrics, upstream probes, health checks |
| Runtime management | Config validation and hot reload, fixed matcher base results, targeted provider reloads, cache and upgrade management |
| Deployment | Multi-platform builds, Debian packages, OpenWrt LuCI app, built-in WebUI and management API, service installation |

See the [Plugin Reference](https://oxidns.org/en/plugin-reference/overview) for the complete list of built-in components and configuration fields.

The WebUI Standard workspace compiles upstream groups, isolated paths, smart routing, dedicated groups, bounded learning, advanced rules, and scenario templates into native OxiDNS YAML in the browser, then applies it through generic validation and transaction APIs. The backend is unaware of Standard/Expert modes and does not control OpenWrt, OS DNS, DHCP, firewalls, or third-party proxies. See [Standard Mode Operations](https://oxidns.org/en/standard-mode-operations).

---

## Quick Start

Install the latest release with one command. By default this installs and starts OxiDNS as a system service:

```bash
curl -fsSL https://oxidns.org/install.sh | sudo sh
```

Elevated Windows PowerShell:

```powershell
irm https://oxidns.org/install.ps1 | iex
```

By default, the installer downloads the matching release, installs the WebUI, and registers and starts the system service.

After installation, verify the management endpoint and the default DNS service:

```bash
curl -fsS http://127.0.0.1:9199/api/health
dig @127.0.0.1 example.com
dig @127.0.0.1 example.com +tcp
```

The WebUI is available at `http://SERVER_IP:9199/` by default. Before allowing access from other devices, enable `api.http.auth` and restrict sources with a firewall or reverse proxy. Run `oxidns check` after changing configuration.

On OpenWrt, the same installer installs [`luci-app-oxidns`](https://github.com/svenshi/luci-app-oxidns). Use the [Quick Start](https://oxidns.org/en/quickstart) for the first successful query; see “Installation & Deployment” in the manual for archives, Docker, Debian packages, portable installs, and uninstallation; see [Custom Build](https://oxidns.org/en/custom-build) to strip optional protocols and plugins.

## Who it is for

OxiDNS fits long-running DNS environments that need fine-grained control and complete observability, including:

- Home gateways, software routers, OpenWrt, NAS, and homelabs
- Policy routing by domain, client, query type, or response result
- Multi-upstream racing, fallback, encrypted DNS, and custom egress
- Ad filtering, local overrides, dual-stack preference, ECS, and dynamic rule learning
- DNS-driven firewall sets, address lists, and policy routes

It is not an authoritative DNS server or a zero-configuration ad-blocking dashboard. If ready-made graphical management is your primary requirement, a product focused on that experience may be a better fit. OxiDNS is for users willing to trade some configuration complexity for control and explainability.

---

## Documentation

- **Get started**: [Quick Start](https://oxidns.org/en/quickstart) · [Configuration](https://oxidns.org/en/configuration) · [Common Scenarios](https://oxidns.org/en/scenarios) · [Migrate from mosdns](https://oxidns.org/en/migrate-from-mosdns)
- **Reference**: [Plugin Overview](https://oxidns.org/en/plugin-reference/overview) · [Management API](https://oxidns.org/en/api) · [CLI](https://oxidns.org/en/cli) · [DNS Codes](https://oxidns.org/en/dns-codes)
- **Deploy and operate**: [Operations & Troubleshooting](https://oxidns.org/en/operations) · [Security Hardening](https://oxidns.org/en/security) · [WebUI](https://oxidns.org/en/webui) · [OpenWrt](https://oxidns.org/en/openwrt)
- **Understand the project**: [Architecture and Design](https://oxidns.org/en/architecture-and-design) · [Performance and Benchmarks](https://oxidns.org/en/benchmarks) · [Documentation Versions](https://oxidns.org/en/documentation) · [Releases](https://oxidns.org/en/releases) · [Roadmap](https://oxidns.org/en/roadmap)
- **Community**: [Contributing](https://oxidns.org/en/contributing) · [Support Project Development](https://oxidns.org/en/support-development) · [GitHub Discussions](https://github.com/svenshi/oxidns/discussions) · [Telegram](https://t.me/oxidns)

## Project Status

OxiDNS is under active development and is suitable for advanced home networks, software routers, homelabs, and other controlled self-hosted environments. Before using it in production or critical networks, validate configuration and fallback behavior, assess capacity, and put monitoring in place. DNS changes directly affect network availability, so keep a recoverable configuration and bypass path.

Issues, real-world feedback, documentation improvements, and plugin contributions are welcome.

---

## Community

Join the Telegram group to chat with the author and other users: [**@OXIDNS** · https://t.me/oxidns](https://t.me/oxidns)

<a href="https://t.me/oxidns">
  <img src=".github/img/telegram-qr.png" alt="OxiDNS Telegram group QR code" width="220" />
</a>

---

## Support Project Development

If OxiDNS is useful to you, you can support its ongoing development and maintenance through WeChat Pay or Alipay. Support is entirely optional, and every contribution is sincerely appreciated.

<table>
  <tr>
    <th align="center">WeChat Pay</th>
    <th align="center">Alipay</th>
  </tr>
  <tr>
    <td align="center"><img src="docs/static/img/support/wechat-pay.jpg" alt="WeChat Pay QR code for supporting OxiDNS development" width="260" /></td>
    <td align="center"><img src="docs/static/img/support/alipay.jpg" alt="Alipay QR code for supporting OxiDNS development" width="260" /></td>
  </tr>
</table>

Financial support does not establish a commercial service relationship or affect feature planning and issue priority. See the [contribution guide](https://oxidns.org/en/contributing) for other ways to help.

---

## License

This project is licensed under the [GNU General Public License v3.0 or later](LICENSE).
