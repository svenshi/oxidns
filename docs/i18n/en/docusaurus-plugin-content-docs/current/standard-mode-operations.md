---
title: Standard Mode Explanation, Diagnostics, and Assets
---

Standard Mode connects intent, the runtime graph, individual queries, and successful history with stable revisions. It still manages only OxiDNS-owned configuration and data files. It never modifies OpenWrt/UCI, OS DNS, DHCP, firewalls, `ipset`, `nftset`, RouterOS, or third-party controllers.

## Compilation explanation

Every Plan returns `plan.generated.explanation` schema 1:

- `intentRevision` is SHA-256 over normalized schema-v6 intent;
- `mappings` connects stable object IDs to Provider, Matcher, Path, Cache, Upstream, and Listener tags;
- `finalPriority` is effective runtime order rather than form order;
- `pathBoundaries` identifies each path's upstream group/members, cache namespace, ECS cache-key behavior, filtering, recording, dual-stack policy, and IP selection;
- `capabilities` lists available and missing optional build abilities;
- top-level `dependency_graph` contains plugin nodes, edges, initialization order, and sequence flows. Generated YAML is read-only; Apply remains the only activation path.

`semantic_diff` schema 1 compares stable objects in the last successful managed intent and the candidate, reporting affected paths, rules, caches, listeners, upstream groups, and managed files. An untrusted baseline is explicitly marked `takeover`.

## Bounded per-query diagnosis

The Standard-generated recorder pins `intentRevision` to each record. The default limit is 512 events; Expert configuration accepts 32–4096. Overflow increments per-query `stepsTruncated` and `droppedStepCount`, so an incomplete trace is never presented as complete evidence.

Facts cover the first failed matcher, default-path reason, fresh/stale/miss/expired cache state, ECS cache separation, fallback branch, stable upstream member selection/timeouts/errors/cancellation, final RCODE/source, total latency, and recorded stage timings.

Events exist only when request-local recording is enabled and remain bounded by event count, upstream fan-out, recorder queue, retention, and reader concurrency. No qname, client, intent revision, or per-query outcome becomes a high-cardinality metric, and upstream credentials are never copied into events.

Existing query-recorder v1 databases receive additive columns and remain readable. Old rows without a revision return raw facts with `explanationUnavailable`; OxiDNS never maps them to a newer intent by guesswork.

## Assets and rollback

- Export uses asset schema 1 and kind `oxidns_standard_intent`, containing complete normalized intent, source schema, build identity, and stable revision.
- Import is limited to 2 MiB and runs existing migration, normalization, validation, semantic Diff, and Plan without writing configuration. Activation still requires reviewed exact-version Apply.
- The latest 20 successful versions use an atomic mode-`0600` history file beside the config. Restore returns intent and reruns Plan/Apply, so a failed candidate cannot replace the last healthy runtime.
- Copy-to-Expert returns detached YAML, version, and dependency graph. It does not switch mode, write files, or retain a false Standard ownership claim.
- Expert analysis only parses, validates, and builds a graph, identifying Expert-only and system-integration plugins. It neither executes configuration nor reverse-compiles arbitrary graphs into lossy Standard intent.
- Saved templates use a local schema-1 asset store beside the config, limited to 64 entries and 2 MiB, with atomic mode-`0600` writes and optimistic versions. Save, duplicate, and delete never mutate runtime; use still passes through server-side collision preview.

See the [Standard Mode Plan/Apply API](api/standard-mode.mdx) for routes and fields.
