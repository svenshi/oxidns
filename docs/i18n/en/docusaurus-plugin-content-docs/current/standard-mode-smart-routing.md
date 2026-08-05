---
title: Standard Mode Smart Routing
sidebar_position: 4
---

Standard Mode schema v5 compiles domestic, remote, and unknown-domain intent into native OxiDNS provider, matcher, sequence, forward, fallback, and cache plugins. It does not read or modify OpenWrt/UCI, OS DNS, DHCP, firewall policy, or any third-party proxy system.

## Scope

“Upstream leak prevention” applies only to queries that reach OxiDNS. Strict-remote mode guarantees that an unknown query cannot execute a domestic or default upstream, but it cannot intercept applications that bypass OxiDNS, hard-coded IP connections, or encrypted DNS sent to another resolver. `outbound` and SOCKS are native OxiDNS networking inputs, not third-party control operations.

## Semantic data roles

| Role | Purpose |
| --- | --- |
| `domestic_domains` | Select the domestic path |
| `foreign_domains` | Select the remote path |
| `domestic_ips` | Validate A/AAAA addresses returned by the domestic path |
| `direct_domains` | Explicitly select the local domestic/direct DNS path |
| `remote_domains` | Explicitly select the remote DNS path |
| `ddns_domains` | Use a short TTL and bypass cache |

Each role can combine manual rules, local text files, online subscriptions, and native `geosite.dat`/`geoip.dat` sources. OxiDNS assumes no subscription URL, filename, or country dataset. Local files must already exist; subscription files are stored in Standard Mode's own data directory.

Every online source receives an independent download, scheduled job, and Provider reload chain. A failed download does not replace the last successful file and does not reload the live Provider. The WebUI Routing page reports not-applied, missing, stale, download-failed, load-failed, and rule-count states, and can refresh one source immediately.

## Unknown-domain modes

| Mode | Initial path | Fallback | Cache boundary |
| --- | --- | --- | --- |
| Compatibility first | Domestic | Remote | Separate `unknown_compatibility` namespace |
| Privacy first | Remote | Domestic only when explicitly enabled | Separate `unknown_privacy` namespace |
| Strict remote | Remote | Never domestic or default | Separate `unknown_strict_remote` namespace |

Domestic and remote paths must differ. Mode selection, response fallback destinations, and semantic paths own separate cache plugins so incompatible policy results cannot cross boundaries.

## Domestic response validation

For A/AAAA queries, the domestic path validates addresses with `resp_ip` and the `domestic_ips` Provider. The following outcomes are explicit and, by default, clear the domestic response before entering remote fallback:

- address outside the domestic IP set: `domestic_ip_mismatch`;
- CNAME-only: `cname_only`;
- NOERROR without the wanted record: `nodata`;
- NXDOMAIN: `nxdomain`;
- SERVFAIL: `servfail`;
- path threshold exceeded: `timeout`;
- upstream execution or network error: `transport_failure`.

A valid domestic address is accepted immediately. Non-address queries do not use IP geography. With query recording enabled, details show the semantic role, initial path, validation result, fallback reason, selected branch, final path, and final upstream group.

## ECS, dual stack, and IP selection

Each path independently supports:

- ECS: inherit, remove, preserve client ECS, derive from the client address, or fixed preset;
- dual stack: no preference, prefer IPv4, prefer IPv6, IPv4-only, or IPv6-only;
- IP selection: `first_success`, `best_within_budget`, or `background`, with bounded probes, concurrency, wait budget, and cache;
- DNSSEC: only `reorder_only` or `skip`; Standard Mode does not expose a mode that removes members of a signed RRset.

ECS runs before cache. Paths that preserve or generate ECS automatically set `ecs_in_key`. IPv4-only/IPv6-only are QTYPE policies rather than address preferences, and `ip_selector` remains separate from upstream racing and dual-stack suppression.

## Pre-apply checks

On save, the browser compiler migrates and normalizes intent, checks build capabilities and references, reports duplicate/overridden/unreachable rules, and generates candidate configuration. Generic `/config/validate` performs the final preflight of native YAML and real relative paths. Only then can the generic recoverable transaction apply it. Schema v4 migrates to v5; legacy ECS and IP-selection placeholders produce review warnings and never silently broaden routing.

See [Standard Mode and Backend Boundary](api/standard-mode.mdx) for the workspace boundary and [Configuration API](api/configuration.mdx) for transaction fields.
