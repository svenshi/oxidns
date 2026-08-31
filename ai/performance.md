# Performance Engineering Guide

This document defines the design and review constraints for
performance-sensitive OxiDNS changes. It focuses on request-path efficiency,
bounded resource use, and operational stability.

## Performance Contract

Optimize the complete configured request path, not only isolated functions:

```text
decode -> server dispatch -> DnsContext -> sequence/matcher/executor/provider
       -> upstream or side effects -> response construction -> encode
```

Performance changes must preserve DNS correctness, feature compatibility, and
lifecycle cleanup. A faster implementation that changes TTL handling, negative
caching, fallback ordering, response completeness, resource limits, or shutdown
safety is a regression.

Developer-machine timing is diagnostic only. Results affected by local CPU,
power policy, scheduling, network state, or background load must not be used as
a release gate or as proof of a project-wide performance change.

## Hot-Path Review

For every performance-sensitive change:

1. Identify the affected request path and the suspected cost: allocation,
   cloning, parsing, hashing, lock contention, queueing, I/O, task scheduling,
   or algorithmic complexity.
2. Move invariant work to startup or plugin initialization where ownership and
   reload semantics allow it.
3. Check that per-request memory, tasks, queues, retained state, and connections
   remain bounded.
4. Keep blocking work and unrelated I/O out of asynchronous request handling.
5. Keep lock scopes small and never hold a lock across `.await` or external
   I/O.
6. Verify cleanup on normal shutdown, reload, cancellation, timeout, and partial
   initialization failure.
7. Run the correctness and integration tests for every affected DNS behavior.
8. Document any tradeoff that increases memory retention, stale state,
   complexity, or operational cost.

Do not add speculative retained state, pooling, sharding, or background tasks
without a concrete ownership model and a clearly identified request-path cost.

## Profiling and Instrumentation

The profiling features declared in the profiling section of `Cargo.toml` expose
diagnostic instrumentation on request-path components. Their feature edges and
names come from the manifest. They are development-only and must not be added
to release bundles or treated as production defaults.

Use instrumentation to locate call paths, allocation ownership, lock pressure,
and unexpected repeated work. Instrumentation overhead changes absolute timing,
so its output is evidence about where work occurs, not an acceptance threshold.

When retaining profiling evidence, record the commit, build profile, enabled
features, runtime configuration, command, sampling duration, and symbolized call
path. Remove temporary instrumentation from normal builds after the diagnosis.

## Design Review Questions

For every hot-path change, ask:

- Can parsing, validation, normalization, or rule compilation happen once
  during initialization?
- Can a borrowed view replace a clone without making ownership fragile?
- Is the data structure sized from realistic demand rather than a configured
  maximum?
- Is shared mutable state necessary, and is its critical section minimal?
- Does a task, queue, retained-state store, or connection pool have an explicit bound and
  saturation behavior?
- Can cancellation or reload leave detached tasks, connections, timers, or
  retained entries behind?
- Does observability introduce high-cardinality labels, repeated formatting, or
  string allocation?
- Can periodic maintenance create request latency spikes?
- Does the optimization weaken correctness, debuggability, or rollback safety?

## Change Acceptance

A performance-sensitive change is ready when:

- Relevant correctness and integration tests pass.
- Request-path work and shared-state ownership are explainable from the code.
- New memory, task, queue, retained-state, and connection growth is explicitly
  bounded.
- Shutdown, reload, cancellation, and failure cleanup remain deterministic.
- Added complexity has a concrete reason and can be maintained independently of
  local timing results.
- User-visible performance claims, if any, are reviewed separately from the
  implementation and are not inferred from a developer workstation.
