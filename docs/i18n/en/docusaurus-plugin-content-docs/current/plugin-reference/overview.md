---
title: Plugin Overview
sidebar_position: 1
---

All OxiDNS capabilities ship as plugins, organized into four layers by responsibility:

- `server`: network ingress; listens for traffic and hands it to the policy entrypoint.
- `executor`: performs actions such as forwarding, caching, rewriting, observability, and system integrations.
- `matcher`: evaluates branch conditions for `sequence`.
- `provider`: supplies reusable domain / IP datasets consumed by matchers and executors.

Complex policies are usually built by composing several plugin types:

```text
server -> sequence
  -> matcher decides
  -> executor acts
  -> provider supplies datasets
  -> upstream or side effect
```

The full built-in plugin catalog is listed below. Click any plugin name to jump to its field reference.

## Server plugins

See [Server Plugins](server.md) for full field reference.

| Plugin | Purpose |
| --- | --- |
| [`udp_server`](server.md#udp_server) | Listens for DNS over UDP and forwards requests to `entry`. |
| [`tcp_server`](server.md#tcp_server) | Listens for DNS over TCP. With `cert` and `key` configured it also serves as a DoT listener. |
| [`http_server`](server.md#http_server) | Provides DNS over HTTPS (DoH) over HTTP/2 with optional HTTP/3. |
| [`quic_server`](server.md#quic_server) | Provides DNS over QUIC (DoQ). |

## Executor plugins

See [Executor Plugins](executor.md) for full field reference. Grouped as: policy orchestration → request handling → response rewriting → observability → side-effect integrations → maintenance.

### Policy orchestration

| Plugin | Purpose |
| --- | --- |
| [`sequence`](executor.md#sequence) | Orchestrates matchers and executors into a pipeline. The most common entry executor. |
| [`fallback`](executor.md#fallback) | Runs a primary executor first and falls back to a secondary executor when the primary is too slow or fails. |

### Request handling

| Plugin | Purpose |
| --- | --- |
| [`forward`](executor.md#forward) | Sends DNS queries to upstreams. |
| [`cache`](executor.md#cache) | TTL-aware response caching with negative cache and persistence support. |
| [`hosts`](executor.md#hosts) | Returns local static `A` / `AAAA` answers using host-style entries. |
| [`arbitrary`](executor.md#arbitrary) | Injects arbitrary DNS records from zone-style rule strings. |
| [`redirect`](executor.md#redirect) | Rewrites a query name toward another target and restores the visible CNAME on the way back. |
| [`ecs_handler`](executor.md#ecs_handler) | Handles EDNS Client Subnet: keep, rewrite, or auto-fill from source IP. |
| [`forward_edns0opt`](executor.md#forward_edns0opt) | Forwards selected EDNS0 options from the request into the final response. |

### Response rewriting

| Plugin | Purpose |
| --- | --- |
| [`ttl`](executor.md#ttl) | Rewrites response TTL values (fixed value or min/max clamp). |
| [`prefer_ipv4` / `prefer_ipv6`](executor.md#prefer_ipv4--prefer_ipv6) | Dual-stack selector: learns presence of the preferred family and suppresses the other. |
| [`black_hole`](executor.md#black_hole) | Returns sinkhole IPs directly for matching `A` / `AAAA` queries. |
| [`drop_resp`](executor.md#drop_resp) | Drops the current response from the context. |
| [`reverse_lookup`](executor.md#reverse_lookup) | Maintains a reverse IP → name cache and optionally answers PTR requests. |

### Observability and debugging

| Plugin | Purpose |
| --- | --- |
| [`query_summary`](executor.md#query_summary) | Emits a concise query summary after downstream execution. |
| [`query_recorder`](executor.md#query_recorder) | Persists requests, responses, and `sequence` path events to SQLite, with history, stats, and SSE stream APIs. |
| [`metrics_collector`](executor.md#metrics_collector) | Collects lightweight request count and latency metrics and exports them in Prometheus format. |
| [`debug_print`](executor.md#debug_print) | Prints request and response objects for debugging. |
| [`sleep`](executor.md#sleep) | Async delay for testing and policy experiments. |

### Side effects and system integration

| Plugin | Purpose |
| --- | --- |
| [`http_request`](executor.md#http_request) | Sends callbacks to external `http/https` services — webhooks, audit, alerts, external integrations. |
| [`script`](executor.md#script) | Runs an external command and injects a stable subset of `DnsContext` as arguments or environment variables. |
| [`ipset`](executor.md#ipset) | Writes response IPs into Linux `ipset` via the embedded netlink backend (no `ipset` binary required). |
| [`nftset`](executor.md#nftset) | Writes response IPs into nftables sets via the embedded netlink backend (no `nft` binary required). |
| [`ros_address_list`](executor.md#ros_address_list) | Syncs response IPs to MikroTik RouterOS `address-list` with dynamic, persistent, and shutdown cleanup support. |

### Maintenance and scheduling

| Plugin | Purpose |
| --- | --- |
| [`upgrade`](executor.md#upgrade) | Triggers the OxiDNS upgrade flow from inside the executor pipeline. |
| [`download`](executor.md#download) | Downloads one or more `http/https` files locally and atomically replaces targets after fully written. |
| [`reload_provider`](executor.md#reload_provider) | Rebuilds selected provider snapshots by tag without triggering a full application reload. |
| [`reload`](executor.md#reload) | Triggers the same application-level full reload as `POST /reload`. |
| [`cron`](executor.md#cron) | Schedules executors in the background via cron expression or fixed interval. |

## Matcher plugins

See [Matcher Plugins](matcher.md) for full field reference.

### Request dimensions

| Plugin | Purpose |
| --- | --- |
| [`qname`](matcher.md#qname) | Matches the query name in the request. |
| [`question`](matcher.md#question) | Matches request questions using provider `contains_question` semantics. |
| [`qtype`](matcher.md#qtype) | Matches request qtypes. |
| [`qclass`](matcher.md#qclass) | Matches request qclasses. |
| [`client_ip`](matcher.md#client_ip) | Matches the client source IP. |
| [`ptr_ip`](matcher.md#ptr_ip) | Decodes the IP from a PTR query name and matches it. |

### Response dimensions

| Plugin | Purpose |
| --- | --- |
| [`resp_ip`](matcher.md#resp_ip) | Matches A and AAAA addresses in response answers. |
| [`cname`](matcher.md#cname) | Matches CNAME targets in the response. |
| [`rcode`](matcher.md#rcode) | Matches the current response code. |
| [`has_resp`](matcher.md#has_resp) | Matches when a response already exists in the context. |
| [`has_wanted_ans`](matcher.md#has_wanted_ans) | Matches when the response already contains answers of the wanted qtype. |

### Context and expressions

| Plugin | Purpose |
| --- | --- |
| [`mark`](matcher.md#mark) | Matches marks already written into the DNS context. |
| [`env`](matcher.md#env) | Matches process environment variables. |
| [`random`](matcher.md#random) | Matches probabilistically for rollout or sampling. |
| [`rate_limiter`](matcher.md#rate_limiter) | Token-bucket rate limiting by client IP. |
| [`string_exp`](matcher.md#string_exp) | General-purpose string expression matcher for cases where dedicated matchers are too rigid. |

### Composition and constants

| Plugin | Purpose |
| --- | --- |
| [`any_match`](matcher.md#any_match) | Composes multiple matcher expressions; returns `true` when any one matches. |
| [`_true`](matcher.md#_true) | Always true. |
| [`_false`](matcher.md#_false) | Always false. |

## Provider plugins

See [Provider Plugins](provider.md) for full field reference.

| Plugin | Purpose |
| --- | --- |
| [`domain_set`](provider.md#domain_set) | High-performance domain rule set, referenced by `qname`, `cname`, and similar plugins. |
| [`geosite`](provider.md#geosite) | Loads one or more codes from the v2ray-rules-dat `geosite.dat` into a reusable domain rule set. |
| [`adguard_rule`](provider.md#adguard_rule) | Provides a reusable subset of AdGuard Home DNS rule evaluation as a provider. |
| [`ip_set`](provider.md#ip_set) | IP / CIDR rule set, referenced by `client_ip`, `resp_ip`, `ptr_ip`, and similar matchers. |
| [`geoip`](provider.md#geoip) | Loads one or more codes from the v2ray-rules-dat `geoip.dat` into a reusable IP / CIDR set. |
