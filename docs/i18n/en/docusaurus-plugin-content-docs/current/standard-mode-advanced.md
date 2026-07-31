---
title: Standard Mode Advanced Policies
---

Standard Mode schema v6 provides dedicated resolution groups, bounded dynamic learning, advanced rules, and complete scenario templates in one backend PolicyPlan. These features only generate native OxiDNS providers, matchers, executors, and UDP/TCP listeners. They never read or modify OpenWrt/UCI, OS DNS, DHCP, firewall state, `ipset`, `nftset`, RouterOS, or third-party controllers.

## Dedicated resolution groups

A dedicated group owns domain rules, an embedded upstream group, filtering/logging/cache/ECS/dual-stack/IP-selection policy, and an optional native UDP/TCP listener. Deleting the aggregate leaves no provider, matcher, forward, path, cache, or listener tag in the next generation. An extra listener only handles traffic explicitly sent to its port; it does not claim to intercept host DNS.

## Dynamic learning

Each profile classifies successful responses with QTYPE, RCODE, wanted-answer, and an optional response-IP role before writing to an isolated `dynamic_domain_set`. Learned routing is below manual allow/block, device, dedicated-group, and manual forced routing. The default `continue` policy uses a bounded asynchronous queue and cannot alter the DNS response; only explicit `fail_closed` propagates a write failure.

Rule and metadata sidecar paths are derived from the profile ID. The maximum-entry limit atomically rejects a new batch at capacity. TTL cleanup expires only learned entries; API corrections are manual entries. Generated tags drive status, paging, correction, clear, pause, and resume operations.

## Advanced rules

Request-phase rules AND-compose domain, client, QTYPE, IANA-timezone periods, and rate-limit-exceeded conditions, then select a path or synthesize a block response. Response-phase rules require exactly one source path and may combine CNAME, RCODE, wanted answer, QTYPE, and response-IP provider checks before rerouting to an isolated target path. Target variants cannot re-enter their source response rule, so the graph is finite.

`fail_open` preserves the original response when the target fails. `fail_closed` returns explicit SERVFAIL or REFUSED. Multi-upstream consensus remains native `forward.response_selection: consensus` and requires at least two enabled upstreams.

## Scenario templates

`low_latency`, `privacy_dns`, `internal_domains`, and `regional_upstream` are expanded deterministically by the backend. Preview returns the complete proposed intent, object diff, diagnostics, generated YAML, tag map, and preflight result. Namespace collisions never overwrite existing objects. Accepting a preview only updates the WebUI draft; the final change still goes through normal Plan/Apply.
