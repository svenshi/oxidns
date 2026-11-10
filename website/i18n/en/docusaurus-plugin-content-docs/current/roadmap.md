---
title: Roadmap
sidebar_position: 5
---

# Roadmap

This page contains unfinished directions only. Shipped capabilities and migration notes belong in [Release Notes](releases.md); GitHub Issues, Pull Requests, and actual releases are the source of truth for concrete work.

The roadmap communicates priority, not release dates or compatibility commitments. Stability, security, performance regressions, and community feedback may change the order.

## Now

### Standard-mode WebUI

Provide a form-driven configuration path for users who do not want to write YAML directly. Common scenarios such as ad blocking, anti-poisoning, family filtering, and split-tunnel acceleration use forms, toggles, and templates while preserving a path to the complete advanced policy editor.

The current focus is generating explainable, validatable configurations that can be switched safely without weakening the underlying `sequence` policy model for the sake of UI simplicity.

## Next

### Plugin management APIs and WebUI integration

Apply the boundary “entity enumeration, status, and actions belong in APIs; counters, histograms, and low-cardinality gauges belong in metrics.” Gradually add runtime management for plugins such as `forward`, `cron`, `download`, `script`, `ip_selector`, `cache`, and `rate_limiter`, then wire those endpoints into WebUI detail panels.

Each API must also address access control, response size, hot-path cost, optional features, and backward compatibility. Endpoint count is not the measure of completion.

## Later

### Cache architecture and performance

Separate lookup, admission, TTL decisions, lazy refresh, eviction maintenance, persistence, and metrics. Reduce message copies, temporary allocations, timestamp write amplification, and lock contention on cache hits, with dedicated large-capacity and concurrent-refresh benchmarks.

### DHCP and local DNS integration

Explore an optional DHCP service with address pools, static leases, lease persistence, and common options. Lease hostname/IP data should reach DNS policy through an explicit data interface; DHCP runtime state must not couple directly to the DNS request hot path.

### Third-party plugin ecosystem

- Explore WebAssembly plugins for independent distribution behind a stable, sandboxed interface.
- Explore native shared-library plugins for workloads with the strictest latency and throughput requirements.
- Before publishing a third-party ABI, stabilize configuration, lifecycle, capability declarations, resource limits, and failure isolation.

## Participate

Read [Contributing](contributing.md) before implementing or claiming a roadmap item, then align the problem, scope, and acceptance criteria through a GitHub Discussion or Issue. Proposals should cover the user scenario, configuration/API impact, performance and security risks, platform scope, and migration strategy.
