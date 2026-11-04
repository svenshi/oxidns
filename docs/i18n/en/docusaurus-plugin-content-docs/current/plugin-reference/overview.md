---
title: Plugin Overview
sidebar_position: 1
format: mdx
---

import PluginAvailability, {BundleGuide} from "@site/src/components/PluginAvailability";

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

<BundleGuide />

## Server plugins

See [Server Plugins](server.mdx) for full field reference.

<div className="plugin-catalog">

| Plugin | Purpose | Included in |
| --- | --- | --- |
| [`udp_server`](server/datagram-stream.mdx#udp_server) | Listens for DNS over UDP and forwards requests to `entry`. | <PluginAvailability from="minimal" compact /> |
| [`tcp_server`](server/datagram-stream.mdx#tcp_server) | Listens for DNS over TCP. With `cert` and `key` configured it also serves as a DoT listener. | <PluginAvailability from="minimal" compact /> |
| [`http_server`](server/encrypted-http.mdx#http_server) | Provides DNS over HTTPS (DoH) over HTTP/1.1, HTTP/2, and optional HTTP/3. | <PluginAvailability from="standard" compact /> |
| [`quic_server`](server/encrypted-http.mdx#quic_server) | Provides DNS over QUIC (DoQ). | <PluginAvailability from="standard" compact /> |

</div>

## Executor plugins

See [Executor Plugins](executor.mdx) for full field reference. Grouped as: policy orchestration → request handling → response rewriting → observability → side-effect integrations → maintenance.

### Policy orchestration

<div className="plugin-catalog">

| Plugin | Purpose | Included in |
| --- | --- | --- |
| [`sequence`](executor/control-flow.mdx#sequence) | Orchestrates matchers and executors into a pipeline. The most common entry executor. | <PluginAvailability from="minimal" compact /> |
| [`fallback`](executor/control-flow.mdx#fallback) | Runs a primary executor first and falls back to a secondary executor when the primary is too slow or fails. | <PluginAvailability from="minimal" compact /> |

</div>

### Request handling

<div className="plugin-catalog">

| Plugin | Purpose | Included in |
| --- | --- | --- |
| [`forward`](executor/resolution.mdx#forward) | Sends DNS queries to upstreams. | <PluginAvailability from="minimal" compact /> |
| [`cache`](executor/resolution.mdx#cache) | TTL-aware response caching with negative cache and persistence support. | <PluginAvailability from="minimal" compact /> |
| [`hosts`](executor/resolution.mdx#hosts) | Returns local static `A` / `AAAA` answers using host-style entries. | <PluginAvailability from="minimal" compact /> |
| [`arbitrary`](executor/resolution.mdx#arbitrary) | Injects arbitrary DNS records from zone-style rule strings. | <PluginAvailability from="standard" compact /> |
| [`response`](executor/resolution.mdx#response) | Builds complete DNS responses from zone-record templates, including sections, RCODE, and response flags. | <PluginAvailability from="standard" compact /> |
| [`redirect`](executor/resolution.mdx#redirect) | Rewrites a query name toward another target and restores the visible CNAME on the way back. | <PluginAvailability from="minimal" compact /> |
| [`client_ip_from_ecs`](executor/resolution.mdx#client_ip_from_ecs) | Replaces the request-local client IP with the ECS address for subsequent matchers and recorders. | <PluginAvailability from="standard" compact /> |
| [`ecs_handler`](executor/resolution.mdx#ecs_handler) | Handles EDNS Client Subnet: keep, rewrite, or auto-fill from source IP. | <PluginAvailability from="minimal" compact /> |
| [`forward_edns0opt`](executor/resolution.mdx#forward_edns0opt) | Forwards selected EDNS0 options from the request into the final response. | <PluginAvailability from="minimal" compact /> |

</div>

### Response rewriting

<div className="plugin-catalog">

| Plugin | Purpose | Included in |
| --- | --- | --- |
| [`ttl`](executor/response.mdx#ttl) | Rewrites response TTL values (fixed value or min/max clamp). | <PluginAvailability from="minimal" compact /> |
| [`ip_selector`](executor/response.mdx#ip_selector) | Actively probes multiple response IPs and selects or reorders them by score. | <PluginAvailability from="standard" compact /> |
| [`prefer_ipv4` / `prefer_ipv6`](executor/response.mdx#prefer_ipv4--prefer_ipv6) | Dual-stack selector: learns presence of the preferred family and suppresses the other. | <PluginAvailability from="minimal" compact /> |
| [`black_hole`](executor/response.mdx#black_hole) | Generates full-qtype interception responses using `nxdomain`, `nodata`, `null`, `custom`, or `refused` mode. | <PluginAvailability from="minimal" compact /> |
| [`drop_resp`](executor/response.mdx#drop_resp) | Drops the current response from the context. | <PluginAvailability from="minimal" compact /> |
| [`reverse_lookup`](executor/response.mdx#reverse_lookup) | Maintains a reverse IP → name cache and optionally answers PTR requests. | <PluginAvailability from="standard" compact /> |

</div>

### Observability and debugging

<div className="plugin-catalog">

| Plugin | Purpose | Included in |
| --- | --- | --- |
| [`query_summary`](executor/observability.mdx#query_summary) | Emits a concise query summary after downstream execution. | <PluginAvailability from="minimal" compact /> |
| [`query_recorder`](executor/observability.mdx#query_recorder) | Persists requests, responses, and `sequence` path events to SQLite, with history, stats, and SSE stream APIs. | <PluginAvailability from="standard" compact /> |
| [`metrics_collector`](executor/observability.mdx#metrics_collector) | Collects lightweight request count and latency metrics and exports them in Prometheus format. | <PluginAvailability from="standard" compact /> |
| [`debug_print`](executor/observability.mdx#debug_print) | Prints request and response objects for debugging. | <PluginAvailability from="minimal" compact /> |
| [`sleep`](executor/observability.mdx#sleep) | Async delay for testing and policy experiments. | <PluginAvailability from="minimal" compact /> |

</div>

### Side effects and system integration

<div className="plugin-catalog">

| Plugin | Purpose | Included in |
| --- | --- | --- |
| [`http_request`](executor/integrations.mdx#http_request) | Sends callbacks to external `http/https` services — webhooks, audit, alerts, external integrations. | <PluginAvailability from="standard" compact /> |
| [`learn_domain`](executor/observability.mdx#learn_domain) | Learns pipeline request domains into `dynamic_domain_set` for dynamic allow or block lists. | <PluginAvailability from="standard" compact /> |
| [`script`](executor/integrations.mdx#script) | Runs an external command and injects a stable subset of `DnsContext` as arguments or environment variables. | <PluginAvailability from="standard" compact /> |
| [`ipset`](executor/integrations.mdx#ipset) | Writes response IPs into Linux `ipset` via the embedded netlink backend (no `ipset` binary required). | <PluginAvailability from="full" compact /> |
| [`nftset`](executor/integrations.mdx#nftset) | Writes response IPs into nftables sets via the embedded netlink backend (no `nft` binary required). | <PluginAvailability from="full" compact /> |
| [`ros_address_list`](executor/integrations.mdx#ros_address_list) | Projects response IPs into a RouterOS `address-list` consumed by firewall, mangle, or policy-routing rules. | <PluginAvailability from="full" compact /> |
| [`ros_route`](executor/integrations.mdx#ros_route) | Projects response IPs as per-IP static routes in a RouterOS routing table with the configured gateway and distance. | <PluginAvailability from="full" compact /> |

</div>

### Maintenance and scheduling

<div className="plugin-catalog">

| Plugin | Purpose | Included in |
| --- | --- | --- |
| [`upgrade`](executor/maintenance.mdx#upgrade) | Triggers the OxiDNS upgrade flow from inside the executor pipeline. | <PluginAvailability from="standard" compact /> |
| [`download`](executor/maintenance.mdx#download) | Downloads one or more `http/https` files locally and atomically replaces targets after fully written. | <PluginAvailability from="standard" compact /> |
| [`reload_provider`](executor/maintenance.mdx#reload_provider) | Rebuilds selected provider snapshots by tag without triggering a full application reload. | <PluginAvailability from="minimal" compact /> |
| [`reload`](executor/maintenance.mdx#reload) | Triggers the same application-level full reload as `POST /reload`. | <PluginAvailability from="minimal" compact /> |
| [`cron`](executor/maintenance.mdx#cron) | Schedules executors in the background via cron expression or fixed interval. | <PluginAvailability from="standard" compact /> |

</div>

## Matcher plugins

See [Matcher Plugins](matcher.mdx) for full field reference.

### Request dimensions

<div className="plugin-catalog">

| Plugin | Purpose | Included in |
| --- | --- | --- |
| [`qname`](matcher/request.mdx#qname) | Matches the query name in the request. | <PluginAvailability from="minimal" compact /> |
| [`question`](matcher/request.mdx#question) | Matches request questions using provider `contains_question` semantics. | <PluginAvailability from="minimal" compact /> |
| [`qtype`](matcher/request.mdx#qtype) | Matches request qtypes. | <PluginAvailability from="minimal" compact /> |
| [`qclass`](matcher/request.mdx#qclass) | Matches request qclasses. | <PluginAvailability from="minimal" compact /> |
| [`client_ip`](matcher/request.mdx#client_ip) | Matches the client source IP. | <PluginAvailability from="minimal" compact /> |
| [`ptr_ip`](matcher/request.mdx#ptr_ip) | Decodes the IP from a PTR query name and matches it. | <PluginAvailability from="minimal" compact /> |

</div>

### Response dimensions

<div className="plugin-catalog">

| Plugin | Purpose | Included in |
| --- | --- | --- |
| [`resp_ip`](matcher/response.mdx#resp_ip) | Matches A and AAAA addresses in response answers. | <PluginAvailability from="minimal" compact /> |
| [`cname`](matcher/response.mdx#cname) | Matches CNAME targets in the response. | <PluginAvailability from="minimal" compact /> |
| [`rcode`](matcher/response.mdx#rcode) | Matches the current response code. | <PluginAvailability from="minimal" compact /> |
| [`has_resp`](matcher/response.mdx#has_resp) | Matches when a response already exists in the context. | <PluginAvailability from="minimal" compact /> |
| [`has_cname`](matcher/response.mdx#has_cname) | Checks whether the response Answer section contains a CNAME record. | <PluginAvailability from="minimal" compact /> |
| [`has_wanted_ans`](matcher/response.mdx#has_wanted_ans) | Matches when the response already contains answers of the wanted qtype. | <PluginAvailability from="minimal" compact /> |

</div>

### Context and expressions

<div className="plugin-catalog">

| Plugin | Purpose | Included in |
| --- | --- | --- |
| [`mark`](matcher/context.mdx#mark) | Matches marks already written into the DNS context. | <PluginAvailability from="minimal" compact /> |
| [`env`](matcher/context.mdx#env) | Matches process environment variables. | <PluginAvailability from="minimal" compact /> |
| [`time`](matcher/context.mdx#time) | Matches by timezone, time window, weekday, and day of month. | <PluginAvailability from="minimal" compact /> |
| [`random`](matcher/context.mdx#random) | Matches probabilistically for rollout or sampling. | <PluginAvailability from="minimal" compact /> |
| [`rate_limiter`](matcher/context.mdx#rate_limiter) | Token-bucket rate limiting by client IP. | <PluginAvailability from="minimal" compact /> |
| [`string_exp`](matcher/composition.mdx#string_exp) | General-purpose string expression matcher for cases where dedicated matchers are too rigid. | <PluginAvailability from="minimal" compact /> |

</div>

### Composition and constants

<div className="plugin-catalog">

| Plugin | Purpose | Included in |
| --- | --- | --- |
| [`any_match`](matcher/composition.mdx#any_match) | Composes multiple matcher expressions; returns `true` when any one matches. | <PluginAvailability from="minimal" compact /> |
| [`_true`](matcher/composition.mdx#_true) | Always true. | <PluginAvailability from="minimal" compact /> |
| [`_false`](matcher/composition.mdx#_false) | Always false. | <PluginAvailability from="minimal" compact /> |

</div>

## Provider plugins

See [Provider Plugins](provider.mdx) for full field reference.

<div className="plugin-catalog">

| Plugin | Purpose | Included in |
| --- | --- | --- |
| [`domain_set`](provider/domain.mdx#domain_set) | High-performance domain rule set, referenced by `qname`, `cname`, and similar plugins. | <PluginAvailability from="minimal" compact /> |
| [`dynamic_domain_set`](provider/domain.mdx#dynamic_domain_set) | Writable local domain rule file with hot-snapshot matching, API management, and learned appends. | <PluginAvailability from="standard" compact /> |
| [`geosite`](provider/domain.mdx#geosite) | Loads one or more codes from the v2ray-rules-dat `geosite.dat` into a reusable domain rule set. | <PluginAvailability from="standard" compact /> |
| [`adguard_rule`](provider/domain.mdx#adguard_rule) | Provides a reusable subset of AdGuard Home DNS rule evaluation as a provider. | <PluginAvailability from="standard" compact /> |
| [`ip_set`](provider/ip.mdx#ip_set) | IP / CIDR rule set, referenced by `client_ip`, `resp_ip`, `ptr_ip`, and similar matchers. | <PluginAvailability from="minimal" compact /> |
| [`geoip`](provider/ip.mdx#geoip) | Loads one or more codes from the v2ray-rules-dat `geoip.dat` into a reusable IP / CIDR set. | <PluginAvailability from="standard" compact /> |

</div>
