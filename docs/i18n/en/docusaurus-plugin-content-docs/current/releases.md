---
title: Release Notes
sidebar_position: 4
---

import ReleaseCard from '@site/src/components/ReleaseCard';

# Release Notes

## 2026-09

<div className="release-stack">
   <ReleaseCard version="v1.6.0" badge="Minor Release" date="2026-09-24" defaultOpen>
       **Release Scope**

       - v1.6.0 updates query-history storage and Windows service recovery, with improvements to scheduled jobs, manual downloads, DNS networking, and WebUI configuration editing.
       - **Read before upgrading**: `query_recorder` v2 history starts empty; v1 history is neither migrated nor deleted automatically. Existing Windows services must be reinstalled to receive the new SCM recovery settings.

       **Changes**

       - `perf(query_recorder)`: v2 shares repeated execution paths and question lists in SQLite and losslessly compresses eligible response snapshots in the background writer. Configuration, query APIs, filtering, statistics, and SSE remain compatible.
       - `fix(service)`: route Windows application restarts through SCM recovery actions configured at installation; fix lost restart signals and service recovery after shutdown.
       - `feat(cron/download)`: add manual scheduled-job execution with result tracking, plus manual download controls and corresponding WebUI actions.
       - `fix(network/ripset)`: improve UDP reply source selection and Windows UDP socket behavior, preserve in-flight DNS work after client disconnects, support ipset protocol 6, and retain nftset lookup errors.
       - `feat(webui)`: improve plugin field layout, preserve YAML formatting during edits, and add confirmation for configuration patches.

       **Compatibility and Upgrade Notes**

       - The root crate is `1.6.0`, `oxidns-proto` is `0.1.6`, and `oxidns-ripset` is `0.1.3`; use release tag `v1.6.0`. Existing YAML configurations upgrade directly. Run `oxidns check -c <config-file>` before replacing the binary.
       - **Query-history migration**: only new v2 records appear after upgrade. Old v1 tables are not read or migrated, and neither WebUI/API “clear history” nor retention cleanup removes them, so they continue to consume disk space. To discard all history and reclaim space, stop OxiDNS and back up the database. Only if `query_recorder.path` points to a file dedicated to that recorder, manually delete the SQLite file and its matching `-wal` and `-shm` files before restarting. If multiple recorders share the file, never delete the whole file; keep it and remove only the target recorder's old tables as described in the plugin reference.
       - **Windows service migration**: in an elevated shell, stop and uninstall the old service, replace the binary with v1.6.0, then reinstall with the original working directory and config path and start it: `oxidns.exe service stop`, `oxidns.exe service uninstall`, `oxidns.exe service install -d <absolute-working-dir> -c <config-file>`, `oxidns.exe service start`. Replacing the binary or restarting the old service alone does not update SCM recovery settings.
   </ReleaseCard>
</div>

## 2026-08

<div className="release-stack">
   <ReleaseCard version="v1.5.2" badge="Patch Release" date="2026-08-18">
       **Release Scope**

       - Patch Release. v1.5.2 focuses on trusted client-IP restoration, isolated dual-stack preference probes, efficient large-rule loading, and safe runtime lifecycles. It also adds sequence mark-set operations and hardens the release pipeline.
       - v1.5.1 YAML configurations upgrade directly. The new `client_ip_from_ecs` executor, `dual_selector.probe_executor` field, and `set_mark` builtin are all opt-in, so existing policies do not change automatically.

       **Changes**

       - `feat(client_ip_from_ecs)`: add an executor that replaces the request-local client IP with an ECS address supplied by a trusted forwarding peer, making that address available to subsequent client-IP matchers, recorders, and policies. Missing or empty allowlists trust IPv4 and IPv6 loopback only, and only complete IPv4 `/32` or IPv6 `/128` host prefixes are accepted. The plugin is included in the standard and full bundles, but not minimal.
       - `feat/fix(dual_selector)`: add optional `probe_executor` support to `prefer_ipv4` and `prefer_ipv6`, allowing preferred-QTYPE probes to use a dedicated `forward` or `sequence`. Omitting it preserves the previous downstream-continuation behavior. Original and probe work use isolated subquery contexts, every return path joins or cancels both tasks, cleanup stops during plugin destruction, and startup rejects missing, wrong-kind, self-referencing, or cyclic dependencies.
       - `feat(sequence)`: let `mark` append multiple `u32` values separated by spaces or commas, and add `set_mark` to replace the entire current mark set. Duplicate values are collapsed, while missing, negative, non-numeric, or overflowing values fail sequence initialization. Dependency graphs, execution paths, and the WebUI editor understand the new syntax.
       - `perf/fix(loaders)`: unify streaming text input and capacity reservation across matchers, hosts, redirect, providers, RouterOS persistence, and zone records, avoiding retention of complete large files or intermediate rule collections. The zone parser gains visitor APIs. Multi-pass compilation fingerprints replayed inputs, publishes only fully built candidates, and moves large compilation work off the async runtime.
       - `fix(runtime/providers)`: provider reload retains serialized ownership after caller cancellation, and runtime teardown drains in-flight reloads and background builds so old snapshot compilation cannot cross reload or destroy boundaries. Replay compilation for AdGuard, V2Ray, and related providers also gains stronger source-location, comment-handling, and rollback coverage.
       - `fix(download/upgrade)`: shared HTTP downloads now own temporary files through drop cleanup, removing incomplete files after timeout, cancellation, or failure and atomically replacing the destination only after success. Windows ZIP-upgrade path handling is corrected as well.
       - `deps/ci/release`: move to `oxidns-mikrotik-rs 0.8.1`, which includes lossless Tokio response delivery, and remove the temporary Git patch plus crates.io `--no-verify`. Update `hotpath`, `base64`, and other dependencies, isolate cross-target build caches, and publish version-bumped workspace support crates in dependency order before the root package.
       - `docs/benchmarks/telegram`: reorganize bilingual installation, configuration, CLI, API, and plugin references; add reproducible multi-implementation benchmark scenarios and results; and render Telegram announcements as compatible HTML that preserves headings, lists, emphasis, inline code, and links, with truncation tests.

       **Compatibility and Upgrade Notes**

       - The root crate version is `1.5.2`; `oxidns-proto` is updated to `0.1.5` and `oxidns-zoneparser` to `0.1.2`; the release tag should be `v1.5.2`. Publication now uploads new support-crate versions before the root crate.
       - v1.5.1 configurations upgrade directly. No fields are renamed or removed, and no existing plugin policy defaults change. Run `oxidns check -c <config-file>` before replacing the binary.
       - `client_ip_from_ecs` changes the request-local client IP observed by later plugins. Place it before the affected matchers and recorders, allow only controlled reverse proxies or local forwarders in `args`, and never trust a source reachable directly by clients. The forwarder must send `/32` or `/128` ECS; network prefixes are ignored.
       - Omitting `dual_selector.probe_executor` preserves v1.5.1 behavior. When configured, probe-context marks, responses, and transient state do not flow back to the original request, but completed external side effects cannot be rolled back; dedicated probe chains should favor side-effect-free resolution executors.
       - Existing single-value `mark` syntax remains valid, and only an explicit `set_mark` clears the previous set. If a large rule file changes during one multi-pass build, the candidate is rejected and the previous snapshot remains active; automation should trigger reload only after the replacement file is fully installed.
   </ReleaseCard>
</div>

## 2026-07

<div className="release-stack">
   <ReleaseCard version="v1.5.1" badge="Patch Release" date="2026-07-22">
       **Release Scope**

       - Patch Release. v1.5.1 focuses on matcher runtime control, upgrade operations, and WebUI quality. It expands temporary matcher switching into tri-state base-result controls, adds force and post-upgrade cleanup controls, and delivers a concentrated set of localization, polling, log-viewer, and plugin-card fixes.
       - Existing YAML configurations upgrade directly, but the matcher runtime management API has a breaking change. Clients using that API must migrate before upgrading.

       **Changes**

       - `feat/fix(matcher)`: replace the runtime switch with `normal`, `always_false`, and `always_true` modes. Both fixed modes skip the matcher implementation and fix its base Boolean value; each `$tag` or `!$tag` reference then applies its own outer negation, so positive and negated results remain opposites. `sequence`, `any_match`, and query recorder now track both the fixed mode and effective match result, with regression coverage for shared controls.
       - `feat(upgrade)`: let the management API and WebUI set `force` to reinstall a release even when it is already current. Add `cleanup` to control removal of download caches and backups after a successful upgrade. The WebUI persists both preferences and generates equivalent CLI commands. Cleanup releases the upgrade lock first and reports cleanup failures without changing a successful apply result.
       - `fix(webui/i18n)`: complete Chinese and English localization for RouterOS, plugin definitions, metrics, configuration history, and console components, including locale-aware date formatting. Add coverage auditing to prevent missing English translations or fallback to Chinese.
       - `fix(webui/runtime)`: schedule runtime polling according to page visibility while retaining background metric collection. Isolate responses, metric baselines, and update-check caches between backend connections; fetch matcher state only on initial load or explicit refresh; reset QPS sampling after long gaps.
       - `feat(webui/logs)`: add persisted timestamp formats, optional elapsed-time display, adaptive duration units, and compact target paths. Unify plugin configuration and metric cards around an adaptive grid, with better RouterOS write-result, timestamp-metric, and system-memory presentation.
       - `perf(build)`: use size-oriented release optimization, fat LTO, one codegen unit, and symbol stripping. Limit Tokio, QUIC, and TLS dependencies to required features, and attempt UPX compression for minimal and standard release artifacts without making compression failure block publication. Exclude development-only benchmarks, site documentation, and WebUI sources from the crates.io source package to stay clear of the registry size limit.
       - `deps/ci/release`: update `wincode`, `syn`, other Rust dependencies, and GitHub Actions. Temporarily apply the RouterOS unbounded-response-channel fix through a Git patch to prevent protocol-event loss under burst traffic. The patch is not published separately, and crates.io publication uses `--no-verify` until upstream ships the fix. GitHub Release and Telegram announcements now share version-heading-validated release notes; announcements target the configured topic and are pinned automatically.

       **Compatibility and Upgrade Notes**

       - The root crate version is `1.5.1`; publishable workspace crate versions remain unchanged, and the release tag should be `v1.5.1`. The RouterOS patch is not published as a separate crate.
       - v1.5.0 YAML configurations upgrade directly. No configuration fields are added, renamed, or given new defaults. Run `oxidns check -c <config-file>` before replacing the binary.
       - **Matcher API migration**: `POST /api/plugins/<matcher_tag>/enable` and `/disable` are removed. Use `POST /api/plugins/<matcher_tag>/mode` with `{ "mode": "normal|always_false|always_true" }`. The `GET /status` response replaces `enabled` with `mode`. Unmigrated automation and third-party controllers will receive a 404 or fail response decoding.
       - Fixed matcher modes exist only in the current runtime and reset to `normal` after an application reload or process restart. The mode is shared by matcher tag, while reference semantics remain local: with `always_false`, `$tag` misses and `!$tag` matches; `always_true` produces the opposite results. Control the `any_match` matcher itself when the whole composition must be fixed.
       - The WebUI defaults to deleting download caches and backups after a successful upgrade. Disable post-upgrade cleanup when local rollback files must be retained. Use `force` only to repair a damaged installation or redeploy the same version, after confirming the intended bundle and platform.
       - Minimal and standard artifacts may be UPX-compressed. Environments with binary scanners, allowlists, or integrity baselines should verify the release asset digest again and smoke-test startup, upgrade, and rollback before production replacement.
   </ReleaseCard>

   <ReleaseCard version="v1.5.0" badge="Minor Release" date="2026-07-19">
       **Release Scope**

       - Minor Release. v1.5.0 centers on RouterOS policy synchronization and live operations: it adds the `ros_route` static policy-route plugin, comprehensively rebuilds `ros_address_list`, and adds management API plus WebUI runtime controls for matchers and providers.
       - It also adds a configurable `response` executor, timezone-aware `time` matching, query-recorder space reclamation, stronger CNAME response selection, advanced WebUI configuration, and broader OpenWrt, ARMv7, and container delivery support.

       **Changes**

       - `feat(routeros)`: add `ros_route` to synchronize observed DNS A/AAAA addresses into per-IP static routes in a selected RouterOS routing table, with IPv4/IPv6 gateways, distance, TTL leases, persistent IP/CIDR entries, optional conntrack-delayed removal, startup recovery, and bounded shutdown cleanup.
       - `refactor(routeros)`: share TLS/API-SSL transport, bounded parallel batching, key-coalescing queues, leases, reconciliation, retry, and lifecycle primitives across `ros_address_list` and `ros_route`. RouterOS outages no longer block DNS startup; synchronous mode is limited by `wait_timeout` and never changes the DNS response on failure; deletion revalidates internal ID, target key, and ownership comment to avoid touching foreign entries.
       - `feat(runtime_control)`: all matchers gain live status, enable, and disable controls; providers gain serialized reload with conflicting concurrent requests rejected. The management API, logs, and WebUI plugin detail panels expose these controls, while builds without the API feature keep them disabled.
       - `feat(response/matcher)`: add a `response` executor that builds Answer, Authority, and Additional sections from zone-record templates with RCODE, flags, and `{qname}`/`{qclass}` placeholders. The `time` matcher gains IANA timezones, multiple periods, overnight windows, weekdays, and month-day constraints.
       - `refactor(query_recorder)`: coordinate readers, writers, and maintenance sharing one SQLite database. Retention cleanup and manual history clearing now delete in batches, truncate WAL, migrate legacy databases to incremental auto-vacuum, reclaim disk space, and keep the in-memory tail consistent with stored results.
       - `fix(dns/forward/cache)`: add a shared query-aware response classifier. Concurrent upstream selection distinguishes complete positives, definitive negatives, and incomplete aliases; bare CNAME responses no longer win early, vote as negatives, or populate address caches. Cache dump/load, lazy refresh, TTL, and persisted-age behavior are hardened while repeated CNAME scans are reduced.
       - `feat(webui)`: add collapsible advanced configuration fields while preserving explicit defaults, `false`, `0`, and empty objects. Replace Monaco with a locally hosted CodeMirror YAML editor with stronger validation, completion, array, and time-period serialization. Improve DNS traffic, process memory, and unavailable-metrics dashboard states.
       - `feat(release/operations)`: add an ARMv7 release target; let the installer deploy `luci-app-oxidns` and its Chinese package on OpenWrt; move the container to an Alpine build stage plus BusyBox musl runtime; split upgrade discovery, digest verification, archive handling, and binary/WebUI installation while correcting full/slim target selection.
       - `fix(config/api/health)`: reject path-unsafe plugin tags and the reserved quick-setup namespace, consistently URL-encode plugin API routes, and report health only after plugin initialization. Synchronize new `response` and RouterOS configuration, features, and build-info capabilities.
       - `deps/ci/docs`: update dependencies and GitHub Actions, including the `oxidns-proto` nightly-Clippy fix. Clarify server, plugin, infra, upgrade, and provider/V2Ray module boundaries and update bilingual API, plugin, OpenWrt, installation, and operations documentation.

       **Compatibility and Upgrade Notes**

       - The root crate version is `1.5.0`; `oxidns-proto` is updated to `0.1.4`; the release tag should be `v1.5.0`.
       - Most v1.4.0 configurations upgrade directly; all newly introduced capabilities are optional. Run `oxidns check` before upgrading. Unsafe plugin tags and reserved quick-setup tags now fail validation and must be renamed together with all references.
       - **RouterOS ownership migration**: the default `ros_address_list.comment_prefix` changes from `fdns` to `oxi`. To continue recognizing, refreshing, or cleaning entries created by older releases, explicitly keep `comment_prefix: fdns` in the existing plugin configuration; handle the old namespace before switching to `oxi`.
       - `ros_address_list` adds optional `tls`, `wait_timeout`, and `queue_capacity`; existing address-list, persistent, and TTL settings remain valid. `cleanup_on_shutdown` still defaults to `true`, and application reload shuts down the old instance first. Deployments requiring policy continuity should evaluate `false` and must not run two processes with the same tag, comment prefix, and target list.
       - `ros_route` is new and requires a pre-created RouterOS routing table/rule plus at least one gateway. `fixed_ttl: 0` creates dynamic routes that never expire naturally, and dynamic leases have no entry-count cap; assess RouterOS routing-table and OxiDNS memory capacity first. Custom builds can select `plugin-ros-address-list` or `plugin-ros-route`; `plugin-mikrotik` remains the aggregate feature.
       - Legacy query-recorder databases migrate to incremental auto-vacuum on the first retention cleanup or manual history clear. The first migration/reclaim of a large database may create noticeable disk I/O, so schedule it outside peak traffic and keep free space available.
       - `forward.concurrent: 1` still does not retry upstreams that were not started. An incomplete CNAME response can still be returned when no better result exists, but is not cached as an address answer. Legacy CNAME-only address entries in cache dumps are discarded during load or hit validation.
       - The container now uses a musl/BusyBox runtime while retaining CA and timezone data. Container, OpenWrt, and ARMv7 deployments should verify startup arguments, mounts, timezone behavior, and upgrade/rollback procedures before production replacement.
   </ReleaseCard>
</div>

## Archive

Detailed notes are partitioned by month so the current page remains bounded:

- [June 2026](releases/2026-06.md)
- [May 2026](releases/2026-05.md)
- [April 2026](releases/2026-04.md)
- [March 2026](releases/2026-03.md)

Before upgrading, read the target release and every intervening “Configuration and Upgrade Notes” section. Historical notes describe behavior at that time; current parameters come from the code and documentation at the matching release tag.
