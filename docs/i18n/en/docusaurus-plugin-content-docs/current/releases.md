---
title: Release Notes
sidebar_position: 4
---

import ReleaseCard from '@site/src/components/ReleaseCard';

# Release Notes

## 2026-06

<div className="release-stack">
   <ReleaseCard version="v1.3.0" badge="Minor Release" date="2026-06-16" defaultOpen>
       **Release Scope**

       - Minor Release. The headline change is turning `black_hole` into a full interceptor that covers every qtype, alongside broad hardening for upstream pools, bootstrap resolution, deadline / cancellation safety, and RouterOS integration. The Rust module layout is also reorganized around new `cli` and `infra` layers while `core` is narrowed to DNS execution semantics. Runtime configuration remains mostly compatible, but `black_hole` no-argument defaults and non-A/AAAA handling changed; Rust library embedders must migrate public module paths.

       **Changes**

       - `feat(executor)`: `black_hole` now supports `mode` (`nxdomain`, `nodata`, `null`, `custom`, `refused`) and applies across all qtypes. With no `ips` it defaults to `nxdomain`; legacy `ips` configurations continue as implicit `custom`.
       - `feat(upstream)`: upstream pools gain `min_conns` for optional warm connections. `max_conns` now has documented range validation, with docs and WebUI schema updates.
       - `fix(upstream)`: pipeline and reuse pools have stronger deadline handling, cancellation safety, slot reclamation, and unusable-connection pruning, reducing hangs and busy retries around connection close, timeout, replacement failure, and upstream recovery paths.
       - `fix(upstream)`: bootstrap servers must be literal IP endpoints, bootstrap answer selection follows valid CNAME chains, and bootstrap queries respect deadlines. HTTP upstream requests also send an `Accept` header.
       - `feat(executor)`: `ros_address_list` exposes `connect_timeout`, `send_timeout`, and `receive_timeout`; RouterOS startup scans and persistent-entry sync now run in the background so slow address lists do not block DNS startup, and cleanup revalidates rows before deletion.
       - `fix(matcher)`: rule-file parsing preserves commas inside line expressions, fixing domain / matcher rules that legitimately contain commas.
       - `refactor`: add `src/cli/` and `src/infra/`; move network, service, upgrade, build_info, errors, tasks, cache, and observability infrastructure under `infra`; keep `core` focused on `context` and `rule_matcher`.
       - `zoneparser`: parse more standard RDATA families directly, including A/AAAA, name records, MX/RT/AFSDB, TXT/SPF/AVC/RESINFO, SOA, SRV, and CAA, while keeping RFC3597 generic syntax fallback.
       - `query_recorder` / internals: extract RDATA JSON serialization and storage helpers to reduce complexity while keeping recorder output paths maintainable.
       - `release`: fix GitHub Actions release artifact uploads so already-packaged archives are not double-archived.
       - `docs(ai)`: centralize maintainer-facing AI / agent notes under `ai/`, add a Chinese GitHub Release template, and make release prep explicitly hand off without automatic commit, tag, or push.

       **Compatibility and Upgrade Notes**

       - Root crate version bumped to `1.3.0`; `oxidns-zoneparser` bumped to `0.1.1`; `crates/macros`, `crates/proto`, and `crates/ripset` do not need version bumps; the release tag should use `v1.3.0`.
       - `v1.2.3` configs generally upgrade directly. Review `black_hole` usage carefully: legacy `ips` configs keep `custom` semantics, no-argument `black_hole` now returns `NXDOMAIN`, and `null` / `custom` return NODATA for non-A/AAAA instead of passing through.
       - Upstream `bootstrap` values must now be `IP:port`, not hostnames. The new `min_conns` option defaults to `0`, so omitted configs keep lazy connection creation.
       - The new `ros_address_list` timeout fields are optional and default-compatible. Large shared RouterOS address lists should still be split into OxiDNS-owned lists to avoid expensive management-plane scans.
       - Rust library embedders must migrate public module paths: old top-level `network` / `build_info` / `upgrade` / `service` and the infrastructure modules previously under `core` now live under `infra`; `core::context` and `core::rule_matcher` remain.
   </ReleaseCard>

   <ReleaseCard version="v1.2.3" badge="Patch Release" date="2026-06-11">
       **Release Scope**

       - Patch Release focused on fixing a high-CPU path where TCP / DoT response writer tasks could spin after `/api/reload`, and on reducing busy retry loops in upstream pools while upstreams are unavailable or restarting. It also adds English WebUI i18n, GitHub token controls for the WebUI upgrade flow, and additional test plus CLI / plugin documentation hardening. No breaking configuration changes.

       **Changes**

       - `fix(server)`: TCP / DoT response writer tasks now exit when the per-connection response channel closes, preventing orphaned writers from spinning after `/api/reload` cancels connection handlers. A regression test covers the closed-channel path.
       - `fix(upstream)`: Pipeline and reuse upstream pools now apply a short backoff when creating a replacement connection fails, avoiding yield-only retry loops during upstream outages or service restarts.
       - `fix(upstream)`: Saturated pipeline pools still retry responsively with scheduler yielding only, so the new backoff stays limited to failed expansion paths.
       - `feat(webui)`: Added English i18n resources and a localization provider for console pages, plugin definitions, help text, and primary WebUI components.
       - `feat(webui)`: Upgrade checks and apply requests can include an optional GitHub token. The WebUI adds explicit persistence controls and risk guidance, while CLI previews avoid exposing tokens.
       - `fix(webui)`: Hide the upgrade header action when the upgrade state is idle.
       - `docs(cli)`: Documented the `build-info` command, including JSON output, capability-matrix fields, and release troubleshooting usage.
       - `docs(plugin)`: Corrected documented default values and kept the Chinese and English plugin docs aligned.
       - `test`: Replaced fixed waits with deterministic synchronization, avoided a Windows cron timer flake, and flushed the query recorder writer before top-clients assertions.

       **Compatibility and Upgrade Notes**

       - Root crate version bumped to `1.2.3`; no workspace crate under `crates/` changed this cycle (`crates/macros`, `crates/proto`, `crates/ripset`, `crates/zoneparser`), so none need a version bump; the release tag should use `v1.2.3`.
       - `v1.2.2` configs upgrade directly to `v1.2.3` with no new required fields or YAML migration.
       - Long-running deployments that use TCP / DoT inbound servers, frequently call `/api/reload`, or observe high CPU while upstream DNS services are restarting or unavailable should upgrade.
       - The WebUI GitHub token is used only for GitHub requests in the upgrade check / apply flow. It can be used for a single session or persisted explicitly; leaving it unset preserves the anonymous-request behavior.
   </ReleaseCard>

   <ReleaseCard version="v1.2.2" badge="Patch Release" date="2026-06-10">
       **Release Scope**

       - Patch Release. The headline addition is an HTTP upgrade API (gated on the `plugin-upgrade` feature) with a WebUI real-time update-available notification, enabling users to detect new releases, compare versions, and trigger the upgrade flow directly from the WebUI. Also fixes the `${VAR}` env-var expansion order (expand after YAML parse, not before) to prevent YAML comment and special-character interference, repairs two WebUI quote-wrap handling issues around `${VAR}` form values, and reliably cleans up zombie connections on H2/H3/DoQ upstreams after the remote peer closes. No breaking configuration changes.

       **Changes**

       - `feat(upgrade)`: Added the HTTP upgrade API (behind the `plugin-upgrade` feature flag). The WebUI gains an update-notification banner that detects available GitHub releases, displays the current vs. latest version comparison, and provides an in-WebUI upgrade entry point.
       - `feat(webui)`: The WebUI upgrade panel now uses the backend plugin-upgrade capability, integrating update detection, upgrade status display, and the upgrade action.
       - `fix(upgrade)`: Fixed the apply state lifecycle and switched to sending all upgrade parameters through the POST body, improving reliability and parameter-passing safety.
       - `fix(api)`: Scoped the upgrade module route registration behind the `plugin-upgrade` feature, preventing builds without the upgrade capability from exposing related endpoints.
       - `fix(config)`: `${VAR}` placeholder expansion now runs after YAML parsing instead of before, fixing interactions where YAML special characters or comment text could interfere with expansion. Also prevents YAML comment content from being treated as expandable text.
       - `fix(config)`: Made `expand_env_in_value_with_lookup` public so external code can use it directly.
       - `fix(webui)`: Fixed two related bugs where the WebUI was incorrectly stripping or preserving quote-wrapping around `${VAR}` placeholder form values.
       - `fix(upstream)`: Zombie H2 (DoH), H3 (DoH3), and DoQ connections are now reliably closed after the remote peer disconnects, preventing connection leaks on long-running deployments.
       - `fix(tests)`: Replaced fixed-duration sleeps with polling in integration tests to reduce spurious flakiness.
       - `fix(doc)`: Corrected the doc-comment formatting for `${qname}`.

       **Compatibility and Upgrade Notes**

       - Root crate version bumped to `1.2.2`; no workspace crate under `crates/` changed this cycle (`crates/macros`, `crates/proto`, `crates/ripset`, `crates/zoneparser`), so none need a version bump; the release tag should use `v1.2.2`.
       - `v1.2.1` configs upgrade directly to `v1.2.2` with no new required fields.
       - The HTTP upgrade API is gated on the `plugin-upgrade` feature and is available only in `standard` / `full` builds; `minimal` builds are unaffected.
       - Deployments using `${VAR}` placeholders in configs where YAML comments appear near the placeholder should upgrade; no config changes are needed and behavior improves automatically.
       - Long-running deployments with H2/H3/DoQ upstreams should upgrade to fix potential connection leaks from zombie connections.
   </ReleaseCard>

   <ReleaseCard version="v1.2.1" badge="Patch Release" date="2026-06-08">
       **Release Scope**

       - Patch Release delivering a WebUI Basic Auth login flow with unified auth management, draggable plugin canvases, and several WebUI interaction fixes (unapplied-plugin warning, numeric type preservation for select fields, all sequence rules visible in the query record flow canvas). Also fixes an upstream connection-pool deadlock after network outage, makes `${VAR}` expansion YAML-quote-aware, and improves `ros_address_list` concurrent write throughput. No breaking configuration changes.

       **Changes**

       - `ros_address_list` performance: pipeline concurrent ROS API write operations and remove the post-add re-query step, reducing latency for large address-list updates.
       - `upstream` connection pool fix: prevent the pool from entering a deadlock state after a network outage, eliminating connection-acquisition stalls on recovery.
       - WebUI: new Basic Auth login flow with a unified auth management entry point; login state persists to `localStorage` with logout and session-restore support.
       - WebUI fix: show an explicit warning when a plugin is staged but not yet applied; suppress 404 noise.
       - Config fix: `${VAR}` env-var substitution now correctly handles placeholders wrapped in YAML quotes, matching the behavior of bare placeholders.
       - WebUI fix: preserve the numeric type of `select` field values on save, preventing silent coercion to string that caused config validation failures.
       - WebUI: plugin canvases support content-keyed draggable layout; canvas positions are persisted per content key.
       - WebUI fix: the query record flow canvas now renders all sequence rules instead of only a subset.
       - Dependencies: batch patch-and-minor Cargo dependency upgrades (2 packages).
       - CI: build environment upgraded to Ubuntu 24.04; added a release artifact collection step.

       **Compatibility and Upgrade Notes**

       - Root crate version bumped to `1.2.1`; no workspace crate under `crates/` changed this cycle (`crates/macros`, `crates/proto`, `crates/ripset`, `crates/zoneparser`), so none need a version bump; the release tag should use `v1.2.1`.
       - `v1.2.0` configs upgrade directly to `v1.2.1` with no new required fields.
       - Deployments with management API auth (`auth`) configured: the WebUI login flow automatically uses the existing Basic Auth credentials — no config changes needed, just refresh the WebUI after upgrading.
       - Deployments using `ros_address_list` with high-volume address writes will see improved concurrent write throughput with no config changes required.
       - Deployments that quote `${VAR}` placeholders in YAML (e.g. `value: "${MY_VAR}"`) will find that expansion now behaves identically to bare placeholders; any extra quoting added to work around the old behavior can be simplified, though the old form remains valid.
   </ReleaseCard>

   <ReleaseCard version="v1.2.0" badge="Minor Release" date="2026-06-03">
       **Release Scope**

       - Minor Release. The headline change is a full compile-time feature system (`minimal` / `standard` / `full` bundles plus granular flags) that gates DoQ / DoH3, DoT / DoH, `api` / `webui` / `metrics`, optional plugins, and TLS / HTTP dependencies behind opt-in features, and exposes the compiled capability set to the CLI, the API, and the WebUI. Two new plugins land in the same cycle: `ip_selector` (response-IP selection) and `dynamic_domain_set` + `learn_domain` (writable dynamic domain sets with online learning). The `env` matcher gains multi-condition support, the WebUI gets drag-and-drop card reordering and a `dynamic_domain_set` rule manager, and several memory / lifecycle bugs in the cache, DoH listener startup, and WebUI upgrade path are fixed. Multiple Cargo dependencies are bumped.
       - Contains one breaking change: the `env` matcher drops the legacy two-token `"KEY" "VALUE"` parsing. Configs that used the old form for equality matching must migrate to `KEY=VALUE` (see upgrade notes below).

       **Changes**

       - Compile-time feature system: new `minimal` / `standard` (recommended default) / `full` bundles covering `server-doq` / `server-doh3` / `server-dot` / `server-doh`, `upstream-doq` / `upstream-doh3` / `upstream-dot` / `upstream-doh`, `api` / `webui` / `metrics`, plus granular plugin flags (`plugin-mikrotik`, `query-recorder`, `ipset`, `cron`, `script`, `download`, `http-request`, `reverse-lookup`, `upgrade`, `arbitrary`, `plugin-ip-selector`, `plugin-dynamic-domain`) and provider flags (`provider-protobuf`, `adguard-rule`). Disabled protocols / plugins referenced from a config now fail with a clear "not compiled in; rebuild with `--features ...`" error. The `minimal` release binary is ~8.9 MB (vs ~21 MB for `full`, ~58% smaller).
       - Release artifacts are bundle-aware: CI and release flows split per bundle, adding Linux musl `minimal` / `standard` archives (the `full` archive name is unchanged). `upgrade` and the installer scripts can resolve a specific bundle. `standard` now bundles `api`, `webui`, `query_recorder`, and `upgrade`.
       - Runtime capability reflection: the CLI and `system/health` API report the active bundle and supported plugin kinds; the WebUI disables unsupported plugin kinds in create, reference picker, card, and detail views.
       - New plugin `ip_selector` (executor): A / AAAA response-IP sorting / filtering with bounded TCP / ping probing, score caching, in-flight probe coalescing, DNSSEC-safe handling, and fail-open fallback. Rejects compatibility aliases and unknown config fields — only native OxiDNS configuration is exposed.
       - New plugins `dynamic_domain_set` (provider) + `learn_domain` (executor): file-backed writable provider with hot snapshots, deduplication, API rule management, and explicit reload; `learn_domain` writes filtered queries / responses into a dynamic domain set without SQLite persistence or a full reload. The WebUI gains a Detail tab to list / add / remove / clear rules for `dynamic_domain_set`.
       - `env` matcher: each argument is now parsed as an independent expression, so a single matcher can express multiple conditions. `KEY=VALUE` is the recommended exact-match syntax; `KEY:VALUE` remains a documented alias; values containing separators stay supported. **Breaking**: the legacy `["KEY", "VALUE"]` two-token form now means "both env vars `KEY` and `VALUE` exist" instead of `KEY == VALUE`.
       - WebUI drag-and-drop card reordering on both the dashboard and the plugin center. The plugin center rewrites the config file's `plugins` order (staged then saved via the 应用更改 pill), as a subset reorder inside the active type tab that preserves other types' relative positions; disabled while a search query is active. Dashboard pinned-card order is a frontend-only preference persisted to `localStorage` and never touches the config file.
       - `ConfigField` gains a `fullWidth` flag (applied to `dynamic_domain_set.path`); fixes uneven config form columns caused by `@container` queries being unable to style their own container.
       - `sequence` step recording is now behind an internal `_sequence-step-recording` feature opted into by `query_recorder`, so builds without the recorder compile the step fields and capture calls out entirely.
       - Cache fixes: treat `size` as an entry limit instead of startup map capacity (no more large up-front allocations for high cache limits); enforce the configured limit immediately after startup and after API dump loads; regression coverage for oversized large-cache dumps.
       - Server fix: pre-flight HTTP/3 feature and TLS requirements before spawning the HTTP/2 listener and clean up partially started HTTP server tasks when startup fails, preventing leaked DoH listener handles.
       - `upgrade` fix: infer the WebUI asset path from the runtime config so it works correctly with `--working-dir` overrides.
       - Config fix: disambiguate runtime placeholders (`{...}`) from `env` placeholders during expansion.
       - `dynamic_domain_set` provider hardening: serialize append staging, write each new rule on its own line, validate rules before writing to disk, keep in-memory structures consistent with file state; skip API route registration when the `api` feature is off.
       - Documentation: new `PLUGIN_DEV.md` plugin development and registration guide, new `SECURITY.md` policy, custom-build docs (zh) and a preset capability matrix in quickstart; a roadmap timeline component; install docs no longer reference GHCR; TLS configuration doc formatting fix.
       - Dependencies: `socket2 0.6.3 → 0.6.4`, `jiff 0.2.24 → 0.2.28`, `wincode 0.5.4 → 0.5.5`, `http 1.4.0 → 1.4.1`, `hyper 1.9.0 → 1.10.1`, `rusqlite 0.39 → 0.40`, `windows-service 0.6 → 0.8.1`.
       - Misc: `IpSelectorCacheConfig` denies unknown fields; fix a runtime test serialization deadlock; clean up cancelled `ip_selector` probes; CI fixes covering minimal / standard / full feature combinations and Windows tests; add a reusable custom-build workflow and a minimal `build.config.yml` example.

       **Compatibility and Upgrade Notes**

       - Root crate version bumped to `1.2.0`; no `crates/` workspace crate changed this cycle (`crates/macros`, `crates/proto`, `crates/ripset`, `crates/zoneparser`), so none need a version bump; the release tag should use `v1.2.0`.
       - `v1.1.4` configs upgrade directly on the default (`full`) or `standard` bundle. If you choose `minimal` or a custom feature subset, references to plugins / protocols that were not compiled in fail at startup with "not compiled in; rebuild with `--features ...`" — add the missing feature or remove the corresponding config entry.
       - **Breaking — `env` matcher**: the legacy two-token form `env: ["KEY", "VALUE"]` (meaning `$KEY == VALUE`) must be migrated to `env: ["KEY=VALUE"]` or `env: ["KEY:VALUE"]`. If you intentionally want the new semantics, confirm that you really mean "both env vars exist". See the migration note in `docs/docs/migrate-from-mosdns.mdx`.
       - For minimized deployments you can build with `--no-default-features --features minimal` (or `standard`); the release channel now ships `minimal` / `standard` / `full` Linux musl archives, and `upgrade` / the installer scripts support explicit bundle selection. Deployments that need WebUI, `query_recorder`, or `upgrade` should stay on `standard` or `full`.
       - Large-cache deployments (e.g. `size > 200000`) should upgrade: previous builds pre-allocated an oversized map and did not strictly enforce the limit after API dump loads; memory now stays aligned with the configured cap.
       - Deployments running DoH without DoH3, or DoH3 without the required TLS configuration, should upgrade: previously a failed HTTP/3 init could leave the HTTP/2 DoH listener leaked. The startup path now pre-validates and cleans up partially spawned tasks.
       - `dynamic_domain_set` / `learn_domain` are gated behind the optional `plugin-dynamic-domain` feature and `ip_selector` behind `plugin-ip-selector`. Both are included in `standard` / `full`; for custom minimal builds enable the corresponding features explicitly.
   </ReleaseCard>
</div>

## 2026-05

<div className="release-stack">
   <ReleaseCard version="v1.1.4" badge="Patch Release" date="2026-05-30">
       **Release Scope**

       - Patch Release reducing memory footprint and reload cost on the provider and rule-matching paths, plus WebUI fixes for mobile config-editor and plugin-filter usability, query-recorder chart labels, and self-hosting the Monaco editor. Also adds a "Migrate from mosdns" guide. This release introduces no breaking configuration changes and leaves the query hot path unchanged.

       **Changes**

       - The `client_ip` / `resp_ip` / `ptr_ip` inline IP matchers now compile via `finalize_compact`, so compiled matchers no longer retain a duplicate copy of the source IP ranges (`ip_set` / `geoip` already did this).
       - `finalize_compact` now moves the merged IPv6 ranges into the compiled matcher instead of cloning them.
       - `geoip` feeds CIDR bytes straight into the matcher via `add_v4_network` / `add_v6_network`, skipping the per-entry `String` format + reparse round trip and speeding up load and reload.
       - `adguard_rule` `badfilter` resolution uses a HashSet built once instead of an O(n²) rescan that reallocated the cache key on every comparison.
       - Fixed the WebUI config editor and plugin filters being unusable on mobile.
       - Fixed Top-N labels being truncated in the WebUI query-recorder charts.
       - The WebUI Monaco editor is now self-hosted instead of loaded from the jsdelivr CDN, working in offline or restricted-network environments.
       - Docs: added a "Migrate from mosdns" guide.

       **Compatibility and Upgrade Notes**

       - Root crate version bumped to `1.1.4`; no `crates/` workspace crate changed, so none need a version bump; release tag should use `v1.1.4`.
       - `v1.1.3` configs upgrade directly to `v1.1.4` with no new required fields.
       - The provider / matcher optimizations are internal implementation changes; they do not alter matching semantics or the query hot path and require no config changes.
       - Deployments using the WebUI config editor in restricted or offline networks benefit from the self-hosted Monaco editor and no longer need external CDN access.
   </ReleaseCard>

   <ReleaseCard version="v1.1.3" badge="Patch Release" date="2026-05-27">
       **Release Scope**

       - Patch Release fixing a Linux `nftset` interval-set ADD/DEL that was rejected by real kernels with EINVAL, an `ipset` byte-order bug in `hashsize` / `maxelem`, and a WebUI `query_recorder` tabs overflow. Also adds an upfront notice on the `black_hole` plugin documentation describing an upcoming behavior redesign. This release does not introduce breaking configuration changes.

       **Changes**

       - Fixed `nftset` ADD / DEL / TEST encoding on interval sets: ADD / DEL now send the two-element list form `nft` userspace uses, resolving the EINVAL rejection observed on real kernels (issue #127); TEST sends only the start key and lets the kernel's interval tree resolve containment. Also fixed the per-element timeout byte order and relaxed dump parsing to tolerate unpaired `INTERVAL_END` anchors.
       - Fixed `ipset` create writing `hashsize` / `maxelem` in native byte order: on little-endian hosts the kernel would read `hashsize=2048` as `524288`. Also removed the stray `IPSET_ATTR_LINENO=0` nested attribute, aligning with libipset.
       - Significantly expanded `ripset` wire-format unit and ipset integration test coverage.
       - Fixed a vertical overflow in the WebUI `query_recorder` detail panel tabs list.
       - Docs: added a prominent notice to the `black_hole` executor section announcing the upcoming `mode` field (`nxdomain` / `nodata` / `null` / `custom` / `refused`) that will cover every qtype, and explaining the motivation; current behavior is unchanged.

       **Compatibility and Upgrade Notes**

       - Root crate version bumped to `1.1.3`; `oxidns-ripset` bumped to `0.1.2`; release tag should use `v1.1.3`.
       - `v1.1.2` configs upgrade directly to `v1.1.3` with no new required fields.
       - Linux deployments using the `nftset` plugin against `flags interval` sets should upgrade promptly; without this fix, ADD / DEL is rejected by real kernels with EINVAL.
       - Linux deployments that let OxiDNS create `ipset` sets with explicit `hashsize` / `maxelem` should upgrade; sets pre-created by the external `ipset` CLI are unaffected by this fix.
       - `black_hole` behavior is unchanged in this release, but the upcoming semantic redesign is worth tracking. For domain-level blocking today, prefer configuring both IPv4 and IPv6 fallback addresses (e.g. `black_hole 0.0.0.0 :: short_circuit`) or use `reject 3`.
   </ReleaseCard>

   <ReleaseCard version="v1.1.2" badge="Patch Release" date="2026-05-27">
       **Release Scope**

       - Patch Release fixing a Linux `nftset` write failure on `flags interval` sets, repairing the Windows service installer, and polishing systemd working-directory semantics, the WebUI run log viewer, and `query_recorder` ranking views. This release does not introduce breaking configuration changes.

       **Changes**

       - Fixed `nftset` decoding set flags with native byte order, which left `is_interval` always false on little-endian hosts and caused every CIDR add against a `flags interval` set to fail with `Unsupported entry for set type`. Flags are now decoded as big-endian, with a byte-order regression test.
       - The `nftset` writer now processes each prefix independently, treats `IpSetError::ElementExists` as a skipped no-op, and aggregates ok / skipped / failed counts into a structured warn log instead of disabling the plugin on a single EEXIST.
       - Fixed the packaged Debian `systemd` unit failing pre-start because of an unwritable `WorkingDirectory`; runtime-relative paths (including WebUI assets) now use `-d/--working-dir` as the single base.
       - Fixed Windows install/uninstall scripts: reworked service management, binary path handling, and uninstall ordering to avoid orphaned processes or stale paths.
       - WebUI run log viewer adds a wrap toggle, and `LogEntry.timestamp` now carries millisecond precision so the UI can show local `HH:MM:SS.mmm` alongside the existing `T+elapsed` column for easier correlation with external timelines.
       - WebUI JSON responses and `query_recorder` SSE streams now tolerate non-JSON errors, heartbeat frames, empty payloads, and malformed events, so transient network hiccups no longer surface console errors.
       - `query_recorder` removes the fixed 200-row cap on top-client, top-qname, and slow-query stats endpoints; the WebUI rankings and slow-query list gain a “load more” control to navigate larger result sets.
       - WebUI plugin field documentation is resynced with the Rust plugin configuration.
       - Documentation site adds a Hero component, refreshes installation steps and the Docker run command, and adds multi-platform quickstart guidance; also documents the Debian `/etc/oxidns` and `/var/lib/oxidns` layout, WebUI symlink behavior, and `client_ip` troubleshooting.

       **Compatibility and Upgrade Notes**

       - Root crate version bumped to `1.1.2`; `oxidns-ripset` bumped to `0.1.1`; release tag should use `v1.1.2`.
       - `v1.1.1` configs upgrade directly to `v1.1.2` with no new required fields.
       - Linux deployments using the `nftset` plugin against `flags interval` sets should upgrade promptly; without this fix, those sets cannot accept any add on little-endian architectures.
       - Deb-package upgrades no longer set systemd `WorkingDirectory`. If you relied on that value to resolve relative paths, set `-d/--working-dir` explicitly instead.
       - Clients of `query_recorder` ranking APIs can now request larger `limit` values; existing 200-row responses parse unchanged, so behavior remains compatible.
   </ReleaseCard>

   <ReleaseCard version="v1.1.1" badge="Patch Release" date="2026-05-25">
       **Release Scope**

       - Patch Release adding `query_recorder` history clearing and tightening the WebUI plugin-deletion workflow. This release does not introduce breaking configuration changes.

       **Changes**

       - Added `DELETE /api/plugins/<tag>/records` for `query_recorder`, clearing persisted query records, execution-path `steps`, and the in-memory tail after flushing the background write queue. The response reports `cleared_records`.
       - Added a “Clear history” action to the WebUI query records panel with a confirmation dialog, clearing-state feedback, and automatic refresh of records, selected detail, and plugin-hit stats after completion.
       - Polished the WebUI plugin delete dialog: wider dependency-impact layout, wrapping for long fields, and clearer source / expected-target / removal-blocker details.
       - Fixed delete-dialog cancellation bubbling into plugin cards and opening the plugin detail sheet.
       - Fixed “repair in editor” removing the plugin before the user edits config; it now only switches to the editor for manual reference handling.
       - Fixed delete icons becoming permanently visible and unclickable when config validation errors are present; the dialog can now open and show the error reason.

       **Compatibility and Upgrade Notes**

       - Root crate version bumped to `1.1.1`; release tag should use `v1.1.1`.
       - `v1.1.0` configs upgrade directly to `v1.1.1` with no new required fields.
       - `query_recorder` history clearing is optional and does not affect existing record capture, retention cleanup, or stats query behavior.
       - Clearing history is irreversible and removes persisted query records plus path events for the selected recorder; on production systems, confirm the audit data is no longer needed before using it.
   </ReleaseCard>

   <ReleaseCard version="v1.1.0" badge="Minor Release" date="2026-05-25">
       **Release Scope**

       - Minor Release focused on safer configuration loading, upgrade and restart handling, `query_recorder` analytics, WebUI operations, and refreshed plugin documentation/navigation. This release includes a breaking `upgrade` configuration change; review related configs or automation before upgrading.

       **Breaking Change**

       - `upgrade` restart configuration changed from enum-style `restart: none|service` and CLI `--restart <none|service>` to boolean `no_restart: true` and `--no-restart`.
       - The default behavior changed as well: successful `upgrade apply` now restarts the service automatically. To keep the old “do not restart after upgrade” behavior, explicitly set `no_restart: true` or pass `--no-restart`.

       **Changes**

       - Configuration loading now supports YAML environment placeholders: `${VAR}`, `${VAR:-default}`, and `$${...}`. Expansion runs during startup, `oxidns check`, management API validation, and save-time validation, includes `include` paths, and reports missing variables or syntax errors with variable name, line, and column.
       - Reworked `upgrade apply` into a cross-platform flow. Windows now supports `.zip` archive extraction, binary replacement, and WebUI directory upgrades; zip extraction rejects unsafe paths to prevent zip-slip.
       - Added GitHub token support for upgrades, useful for higher API rate limits or private repositories. CLI uses `--github-token`; plugin configuration uses `github_token`.
       - Successful upgrades now restart by default. CLI upgrades restart the installed service through the platform service manager, while plugin-triggered upgrades request a graceful in-process restart that loads the new binary. To skip restart, use CLI `--no-restart` or plugin config `no_restart: true`.
       - Added management control `POST /restart`. On Unix the process restarts in place with `exec`; on Windows service deployments it cooperates with SCM restart behavior. OxiDNS also captures the original executable path before binary replacement so Linux restarts do not fail on `/proc/self/exe (deleted)`.
       - `query_recorder` gained aggregate stats APIs and WebUI charts: top clients, top qnames, qtype / rcode distributions, latency histogram, slow-query ranking, and minute/hour query trends. SQLite read/write settings were tuned for these aggregation queries.
       - WebUI config lifecycle is clearer: top-level `runtime` / `api` / `log` changes now prompt for restart instead of hot reload; config rollback chooses hot reload or restart based on the changed fields, and restart progress is shown while the console waits for reconnection.
       - WebUI plugin management now checks references before deletion, can replace references, remove safely removable references, or jump to the editor for manual repair. Plugin renames update references and ask for confirmation when other plugins are affected.
       - Documentation refreshed the plugin overview and sidebar navigation, added the roadmap page, and clarified `redirect` rule forms, `qname` / `cname` domain rules, README roadmap, and disclaimers.

       **Compatibility and Upgrade Notes**

       - Root crate version bumped to `1.1.0`; release tag should use `v1.1.0`.
       - `v1.0.2` DNS resolution configs generally upgrade directly to `v1.1.0`; environment placeholders are additive and do not affect configs that do not use them.
       - Breaking Change: old `restart: none|service` and CLI `--restart <none|service>` are no longer accepted. Use `no_restart: true` / `--no-restart` instead. To preserve the old “do not restart after upgrade” behavior, set `no_restart: true` or pass `--no-restart`.
       - Missing `${VAR}` placeholders now fail config parsing. Use `$${...}` for literal `${...}`, and quote placeholders when environment values may contain YAML-special characters.
       - New `github_token` / `--github-token` support is optional and does not affect existing public-repository upgrade configs.
       - Existing `query_recorder` deployments can use the new stats APIs and WebUI charts without config changes. The stats endpoints read SQLite history, so large databases should be monitored for disk and query latency.
   </ReleaseCard>

   <ReleaseCard version="v1.0.2" badge="Patch Release" date="2026-05-21">
       **Release Scope**

       - Patch Release fixing domain-based upstreams depending on local DNS during startup and config validation, and clarifying `bootstrap` versus `dial_addr` resolution precedence.

       **Changes**

       - Fixed `forward` address validation reusing full `ConnectionInfo` construction. Domain-based upstreams now perform syntax validation only and no longer trigger system DNS resolution during startup validation.
       - Changed upstream connection-info construction so only literal IPs and explicit `dial_addr` values become startup-known remote IPs; hostnames remain as `server_name` and are resolved later through `bootstrap` or at first connection time.
       - Clarified the runtime mutual exclusion between `dial_addr` and `bootstrap`: when both are configured, `dial_addr` takes precedence, `bootstrap` is ignored, and initialization emits a warning.
       - Updated `forward` plugin reference docs and WebUI field descriptions with hostname resolution timing, the `bootstrap` / `dial_addr` either-or recommendation, and precedence behavior.
       - Added regression coverage for deferring domain upstream resolution, preserving the SNI hostname with `dial_addr`, and `dial_addr` overriding `bootstrap`.

       **Compatibility and Upgrade Notes**

       - Root crate version bumped to `1.0.2`; release tag should use `v1.0.2`.
       - `v1.0.1` configs upgrade directly to `v1.0.2` with no new required fields.
       - Domain-based upstreams without `bootstrap` or `dial_addr` no longer block startup; the first connection still uses the operating system resolver.
       - To avoid runtime dependence on local DNS entirely, configure exactly one of `bootstrap` or `dial_addr` for domain-based upstreams.
       - Existing configs that set both `bootstrap` and `dial_addr` still start, but only `dial_addr` is effective.
   </ReleaseCard>

   <ReleaseCard version="v1.0.1" badge="Patch Release" date="2026-05-20">
       **Release Scope**

       - Patch Release fixing DNS response compliance issues, client-IP canonicalization, and WebUI usability problems from `v1.0.0`, while adding service management capabilities, installer scripts, and query-auditing UX improvements.

       **Changes**

       - Fixed `redirect` plugin placing synthetic CNAME records after other answers instead of first, aligning with RFC expectations.
       - Fixed dual-stack sockets passing IPv4-mapped IPv6 addresses (`::ffff:x.x.x.x`) into `DnsContext` without canonicalization, causing `client_ip` matchers and other IP-dependent logic to misidentify them as IPv6.
       - Fixed WebUI returning 404 on page refresh and added auto-connect to `/api` on first load for full-backend-hosting deployments.
       - Added `service restart` CLI command for restarting OxiDNS when running as a system service.
       - Added Linux / macOS / Windows hosted-service installer scripts (`install.sh` / `install.ps1`) for one-command install, registration, and service startup.
       - `query_recorder` panel gained click-to-filter by matcher row, color-coded latency badges, and Info tooltips on record-count column headers.
       - WebUI plugin detail sheets, cache dialogs, and config field editor polished: replaced native `confirm` with shadcn AlertDialog, adopted responsive two-column grid layout, and constrained content to `max-w-6xl` centered layout.

       **Compatibility and Upgrade Notes**

       - Root crate version bumped to `1.0.1`; release tag should use `v1.0.1`.
       - `v1.0.0` configs upgrade directly to `v1.0.1` with no new required fields.
       - Deployments using dual-stack sockets with `client_ip` matchers, ECS, or IP-dependent policies will see client IPs correctly canonicalized to IPv4 after upgrading.
       - Installer scripts register the app as a system service by default; set `OXIDNS_INSTALL_SERVICE=0` for portable-only installation.
   </ReleaseCard>

   <ReleaseCard version="v1.0.0" badge="Major Release" date="2026-05-19">
      **Release Scope**

      - Major Release marking OxiDNS's move from an experimental plugin-driven DNS engine to the 1.0 stable line. `v1.0.0` officially includes the built-in WebUI management console and management API, plugin runtime, observability, packaging, and stability work since `v0.5.2`.

      **Important Upgrade Notice**

      - This release completes the project rename to OxiDNS. The GitHub repository, release assets, binary name, package metadata, service files, README files, docs site, logo, and startup banner now use `oxidns`.
      - Older automatic upgrade flows still point at the pre-rename project and release assets, so they cannot upgrade directly to `v1.0.0`. When upgrading from an older build, manually download the matching OxiDNS release package, replace the binary, and deploy the bundled WebUI static assets.
      - After this one-time manual migration, future upgrades should use the new `svenshi/oxidns` repository and `oxidns-*` release assets.

      **WebUI Capabilities**

      - OxiDNS WebUI brings runtime status, configuration, plugins, metrics, logs, query auditing, and cache management into one console, reducing day-to-day reliance on scattered CLI commands, log files, and handwritten API calls.
      - Configuration management is safer for production use: YAML editing, live validation, config history, diff review, apply, and rollback live in one workflow, so complex policy changes can be reviewed before they take effect and recovered more easily.
      - Plugin orchestration is easier to understand: plugin topology, plugin details, structured configuration, and the sequence composer make the DNS request path visible and reduce YAML reference mistakes, missing dependencies, and troubleshooting time.
      - Troubleshooting is more direct: metrics, live logs, query records, execution flow, and cache details can be inspected together, making it easier to trace a problematic domain to matched rules, upstream behavior, cache state, and final responses.
      - Deployment and access are simpler: the WebUI ships with release archives, Docker images, Debian packages, and the `upgrade` flow, and OxiDNS can host the static assets directly from the management API so one process serves both the API and the console.

      **Changes**

      - Reworked the management API into a prefixed unified entry point, adding auth/CORS support, runtime state, log streaming, metrics, config save/apply/rollback, and plugin API aggregation.
      - Replaced the mutable global plugin registry with an immutable catalog plus runtime manager, simplified plugin factory creation context, hardened reload paths, and split registry internals into catalog, context, init_plan, and runtime modules.
      - Added a shared plugin metrics layer covering servers, forward upstreams, cache, query recorder, and side-effect executors, with unified management API exposure.
      - Expanded `query_recorder` with sampled matcher-hit stats, filtering, execution-flow visualization, record details, and cleaned-up model/store structures.
      - Added cache management APIs for reading cached DNS response details, TTLs, hit metadata, record contents, and cache snapshots.
      - Updated `upgrade`, release, Docker, Debian packaging, systemd service files, and CI workflows for the OxiDNS 1.0 release path.
      - Completed the project branding migration from ForgeDNS to OxiDNS across GitHub templates and all user-facing project identity.
      - Performance and stability work includes disabling Nagle on TCP upstreams, reducing split-lock pressure, moving dual-selector preferred probing out of forward, supporting dual-stack port-only listeners, and hardening global runtime-manager reloads.
      - Refreshed README, quickstart, configuration, API, plugin reference, scenarios, benchmarks, and MikroTik policy routing documentation.

      **Compatibility and Upgrade Notes**

      - The root crate version is now `1.0.0`; the release tag should be `v1.0.0`.
      - Existing `v0.5.2` DNS resolution configs should generally upgrade directly. This release mainly introduces the complete WebUI, management API, metrics, and packaging capabilities.
      - When upgrading from pre-rename builds, do not rely on the old automatic upgrade flow to cross the rename boundary. Manually download the `v1.0.0` release package and complete the migration.
      - The management API now uses a prefixed route layout. If reverse proxies, ACLs, or automation scripts call old API paths directly, confirm them against the updated API docs.
      - For automatic upgrade, Docker, or Debian package deployments, confirm that the console static asset directory and service files are installed with the new package.
      - Deployments relying on plugin reload, online config editing, or runtime APIs should validate auth, CORS, permissions, and rollback flows in a staging environment first.
   </ReleaseCard>

   <ReleaseCard version="v0.5.2" badge="Patch Release" date="2026-05-04">
      **Release Scope**

      - Patch Release focused on DoH / DoH3 upstream long-connection reuse and upstream duration parsing.

      **Changes**

      - Fixed an issue where DoH (HTTP/2) and DoH3 (HTTP/3) upstream connection pools could reuse already closed connections. After the remote peer closes an idle connection, the pool now evicts unavailable connections and recreates fresh ones, avoiding repeated `H2 send_request error` or `H3 send_request error` failures (Closed #78).
      - Fixed upstream `timeout` configuration parsing. Values such as `timeout: 3` and `timeout: "3s"` now deserialize correctly and can be used during forward plugin initialization (Closed #79).
      - Added unified duration parsing for duration-based fields, supporting units such as `ms`, `s`, `m`, `h`, and `d`. Bare numeric values are interpreted as seconds by default.

      **Compatibility and Upgrade Notes**

      - This release does not introduce new required configuration fields. Existing `v0.5.1` configurations can be upgraded directly.
      - Duration-based fields such as `timeout` and `idle_timeout` support formats including `3`, `"3"`, `"3s"`, and `"500ms"`.
      - Bare duration numbers are interpreted as seconds. Use an explicit `ms` suffix for millisecond-level values.
      - Upgrading to `v0.5.2` is recommended for deployments that configure upstream `timeout`, or use DoH / DoH3 upstreams and have seen repeated request failures after long runtimes.
  </ReleaseCard>
</div>

## 2026-04

<div className="release-stack">
  <ReleaseCard version="v0.5.1" badge="Patch Release" date="2026-04-28">
      **Release Scope**

      - Patch Release focused on `any_match` quick-setup dependency analysis and `query_recorder` pagination and cleanup boundaries.

      **Changes**

      - Fixed `any_match` dependency analysis so quick-setup matcher expressions are preserved and expanded correctly. Expressions such as `qname $provider` and `qtype 1` now keep their original meaning during startup and dependency analysis.
      - Fixed `query_recorder` retention cleanup and pagination cursor boundaries. Cleanup cutoff time now uses real timestamps, and paginated listing fetches one extra row to detect whether another page exists.
      - Adjusted `query_recorder` timestamp storage and read paths to avoid unnecessary unsigned conversions around record creation time.
      - Updated the `upgrade` CLI default cache and backup directories to `./upgrade-cache` and `./upgrade-backups`, and fixed the matching default-value tests.

      **Compatibility and Upgrade Notes**

      - This release does not introduce new configuration fields. Existing `v0.5.0` configurations can be upgraded directly.
      - Upgrading to `v0.5.1` is recommended for deployments that use `query_recorder` or quick-setup expressions inside `any_match`.
      - `query_recorder` remains **Experimental**, and its API surface or configuration fields may still change in future releases.
  </ReleaseCard>

  <ReleaseCard version="v0.5.0" badge="Minor Release" date="2026-04-27">
      **Release Scope**

      - Minor Release adding query auditing, aggregate matcher support, and HTTP/3 discovery improvements.

      **Changes**

      - Added the `query_recorder` executor for persisted query logging with retention cleanup, plus plugin API endpoints for stats, paginated record listing, and single-record details.
      - Added the `any_match` matcher so one matcher can aggregate multiple matcher expressions and return true when any branch matches, including negated expressions like `!$tag`.
      - When HTTP/3 is enabled on the HTTP server, HTTP/2 responses now automatically advertise `Alt-Svc: h3=":<listen-port>"; ma=86400` so clients can discover and upgrade to H3.
      - Fixed dependency tracking for negated matchers, for example `!$has_resp`, inside `sequence`, so quick setup and dependency analysis no longer miss those references (Closed #75).
      - Unified time handling around `jiff + AppClock`, making cron scheduling, log time formatting, and system-time access paths more consistent.

      **Compatibility and Upgrade Notes**

      - This release does not require global config migrations. Existing `v0.4.x` configurations can be upgraded directly.
      - `query_recorder` is currently **Experimental**. Its API surface and configuration fields may change in upcoming minor releases.
      - To enable query auditing, insert `query_recorder` into the `sequence` chain and tune retention parameters according to disk budget.
      - For automatic HTTP/3 discovery by DoH clients, ensure `enable_http3: true` is set and TLS certificate/key are configured correctly.
  </ReleaseCard>

  <ReleaseCard version="v0.4.2" badge="Patch Release" date="2026-04-24">
      **Release Scope**

      - Patch Release fixing connection release in upstream race scenarios and adding automatic upgrade support.

      **Changes**

      - Fixed an issue where some connections were not properly released in upstream race scenarios, such as when multiple concurrent upstreams were configured or fallback was enabled.
      - Added the `upgrade` CLI tool and plugin to support automatic updates and binary replacement.
      - When the application is running as a Linux service, `upgrade` can also restart it automatically after the update.

      **Compatibility and Upgrade Notes**

      - This release does not introduce new required configuration fields.
      - Deployments that rely on concurrent upstream racing, fallback, or automatic upgrade flows can upgrade to `v0.4.2`.
  </ReleaseCard>

  <ReleaseCard version="v0.4.1" badge="Patch Release" date="2026-04-23">
      **Release Scope**

      - Patch Release fixing an upstream `request_map` memory leak and improving DoH HTTP response compatibility.

      **Changes**

      - Fixed an upstream `request_map` memory leak during connection close, request timeout, and abnormal cleanup paths, preventing pending query waiters and senders from being retained over time.
      - Reworked `request_map` into a fixed-capacity sparse table so each connection no longer reserves the full `u16` DNS ID space.
      - Fixed DoH response header generation so `application/dns-message` replies carry the correct `Content-Length`, and `Cache-Control: max-age=...` is derived from the actual DNS TTL.
      - Common `NoError`, `NXDOMAIN`, and `NODATA` DoH responses now derive HTTP cache lifetime from answer TTLs or SOA negative TTLs. Refusal-style replies no longer advertise misleading cache headers.

      **Compatibility and Upgrade Notes**

      - This release does not add new configuration fields. Existing `v0.4.0` configs can be upgraded directly to `v0.4.1`.
      - Because this release fixes an upstream `request_map` memory leak, upgrading to `v0.4.1` is recommended for long-running deployments with many persistent or concurrent upstream connections.
      - For DoH access through `dig +https://...`, browsers, reverse proxies, or HTTP caches, the upgrade also improves HTTP response compatibility.
  </ReleaseCard>

  <ReleaseCard version="v0.4.0" badge="Minor Release" date="2026-04-19">
      **Release Scope**

      - Minor Release adding provider-scoped hot reload and reworking provider composition and initialization.

      **Changes**

      - Added the `reload_provider` executor plus the provider-scoped management API `POST /plugins/<provider_tag>/reload`. After downloading or overwriting rule files, OxiDNS can refresh only the affected providers instead of forcing a full application reload.
      - Reworked provider composition so `domain_set` and `ip_set` compile only their own local rules and keep querying referenced providers from `sets` at runtime.
      - Runtime initialization now skips providers that have no live dependents, so unused rule sets no longer spend startup time on file reads, dat parsing, or memory allocation.
      - Expanded quick-setup dependency analysis into runtime reference paths such as `sequence` and `cron`, making plugin dependency graphs and init ordering more accurate.
      - Added docs for targeted provider reload through both the API and the new `reload_provider` executor, including chained download-and-refresh examples.

      **Compatibility and Upgrade Notes**

      - Existing workflows that run `download` and then a full `reload` can usually switch to `download -> reload_provider` to avoid rebuilding unrelated plugins.
      - `reload_provider` only refreshes an existing provider's config snapshot and external data files. If `config.yaml`, provider tags, `sets` topology, or the plugin list changes, keep using the full `reload` path.
      - Providers that are not reachable from any live runtime path are no longer inserted into the runtime registry. Deployments that rely on a provider's runtime API surface or behavior must reference it directly or indirectly from a live `server`, `executor`, or `matcher`.
  </ReleaseCard>

  <ReleaseCard version="v0.3.2" badge="Patch Release" date="2026-04-16">
      **Release Scope**

      - Patch Release reducing false warning logs from normal connection lifecycles and improving debug output.

      **Changes**

      - Adjusted UDP, TCP, DoT, and DoQ upstream pool initialization so OxiDNS no longer pre-creates idle connections during startup, reducing false EOF / reset warnings when upstreams close idle sockets on their own.
      - Expected TCP upstream lifecycle events such as EOF, connection recycling, and invalid-connection eviction are now logged at `debug` instead of `warn`.
      - Downgraded DoH server-side TLS, HTTP/2, and HTTP/3 handshake aborts plus client-closed response-send failures to `debug`.
      - Debug request/response logging now prints DNS `questions`, message IDs, EDNS data, and answers directly. `Record` now has a more readable `Debug` / `Display` representation.

      **Compatibility and Upgrade Notes**

      - This release does not introduce new configuration fields. Existing `0.3.x` configs can be upgraded as-is.
      - Warning-count based alerting should see a noticeable drop in noise after `v0.3.2` because normal upstream disconnects and DoH client aborts are no longer treated as warnings.
  </ReleaseCard>

  <ReleaseCard version="v0.3.1" badge="Patch Release" date="2026-04-14">
      **Release Scope**

      - Patch Release fixing `sequence` builtin control-flow semantics and completing release metadata.

      **Changes**

      - Fixed `sequence` builtin control-flow semantics so `accept` / `reject` stop the current chain consistently, `return` explicitly resumes the caller, and nested `jump` / `goto` behavior is more consistent.
      - Removed the old internal flow-state dependency from control-flow propagation and now relies on `ExecStep` directly, reducing ambiguity when `sequence`, `with_next` executors, and nested calls are combined.
      - Expanded unit and integration coverage around `sequence`, including `accept`, `return`, `reject`, `jump`, `goto`, and `adguard_rule` / `question` driven branches.
      - Added the metadata, README files, repository links, and versioned dependency declarations needed to publish `oxidns-proto`, `oxidns-zoneparser`, and `oxidns-ripset` to crates.io cleanly.
      - Refreshed the `configuration`, `executor`, and `matcher` docs to explain builtin `sequence` control flow, `mark` syntax, and numeric `qtype` / `qclass` forms more clearly.

      **Compatibility and Upgrade Notes**

      - For policy layouts that depend on nested `sequence` calls or `jump` / `goto` / `return` combinations, `v0.3.1` is the recommended upgrade for predictable control-flow behavior.
      - This release does not introduce new config fields; it focuses on control-flow fixes, test hardening, and release metadata cleanup.
  </ReleaseCard>

  <ReleaseCard version="v0.3.0" badge="Minor Release" date="2026-04-14">
      **Release Scope**

      - Minor Release adding HTTP callbacks, config checking, dat export, zone parsing, and Linux netlink integration.

      **Changes**

      - Added the `http_request` executor for synchronous or asynchronous `http/https` callbacks in either the `before` or `after` phase, with template placeholders, `json/form/body` payloads, SOCKS5, redirect handling, and configurable error modes.
      - Added the `check` and `export-dat` CLI commands. `check --graph` performs static validation and prints the plugin dependency graph, while `export-dat` can export selected rules from `geosite.dat` / `geoip.dat` into OxiDNS or original text formats.
      - Aligned `hosts` behavior with mosdns semantics, and upgraded `arbitrary` with a fuller zone parser that supports `$ORIGIN`, `$TTL`, `$INCLUDE`, `$GENERATE`, RFC3597, and broader record syntax.
      - Switched the Linux `ipset` / `nftset` executors to an embedded Rust netlink backend, removing the runtime dependency on the `ipset` / `nft` shell commands.
      - Split protocol, zone parsing, and Linux integration internals into three workspace crates: `oxidns-proto`, `zoneparser`, and `ripset`. Added a reusable wire-buffer pool on the network hot path and tuned UDP/TCP/upstream socket parameters.
      - Added a dedicated CLI docs page and refreshed the `executor`, `provider`, `quickstart`, `benchmarks`, and `releases` chapters.

      **Compatibility and Upgrade Notes**

      - Unprefixed `hosts` rules now behave as `full:` rules; positive local answers now use a fixed TTL of `10`; and a name hit without a matching address family now returns `NoError + empty answer + fake SOA` instead of falling through the rest of the executor chain.
      - `arbitrary` no longer provides the old quick-setup syntax. Migrate those cases to explicit `rules` / `files` configuration when upgrading.
      - Quickstart added a Docker Compose example and clarified Docker image registry, Windows release assets, and service deployment guidance.
  </ReleaseCard>

  <ReleaseCard version="v0.2.1" badge="Patch Release" date="2026-04-03">
      **Release Scope**

      - Patch Release fixing DoH over HTTP/2 upstream GET handling and updating quickstart documentation.

      **Changes**

      - Fixed a DoH over HTTP/2 bug where GET requests did not close the request stream, causing some upstreams to time out after 5 seconds.
      - Completed the `Question` `Display` implementation so logs and debug output render DNS questions consistently.
      - Relaxed the cache TTL unit test to tolerate cross-second timing drift in CI.
      - Removed the Docker `linux/arm/v7` support note from quickstart and added a `docker compose` deployment example.

      **Compatibility and Upgrade Notes**

      - This release does not introduce new configuration fields.
      - Upgrading to `v0.2.1` is recommended for deployments using DoH over HTTP/2 upstream GET requests.
  </ReleaseCard>

  <ReleaseCard version="v0.2.0" badge="Feature Release" date="2026-04-02">
      **Release Scope**

      - Feature Release adding subscription download, scheduled jobs, script execution, and geodata provider support.

      **Changes**

      - Added the `download` executor for downloading remote `http/https` files to local storage, with SOCKS5 proxying, HTTP redirect following, and startup bootstrap for missing files.
      - Added the `cron` executor for background jobs with interval or standard 5-field cron triggers.
      - Added the `reload` executor for full application reloads.
      - Added the `script` executor for running external commands with injected context fields.
      - Added `geoip`, `geosite`, and `adguard_rule` providers, plus the `question` matcher. Extended `qname` matching to support `adguard_rule` rule sets directly.
      - Cache now supports stale lazy refresh, rule matcher internals were split and optimized, and configurable log file rotation was added.
      - Expanded documentation for `executor`, `matcher`, `provider`, `server`, `quickstart`, and `scenarios`, and added docs-site CI.

      **Compatibility and Upgrade Notes**

      - `startup_if_missing` is enabled by default for smoother first-deployment and rule-file bootstrap behavior.
      - `ros_address_list` supports `fixed_ttl=0` for no-timeout behavior.
      - Added `short_circuit` support to quick setup for `hosts`, `black_hole`, and `cache`.
      - Removed the `hosts` quick setup to tighten early quick-setup behavior.
      - Migrated from `serde_yml` to `serde_yaml_ng`, with several dependency and CI tooling updates.
  </ReleaseCard>
</div>

## 2026-03

<div className="release-stack">
  <ReleaseCard version="v0.1.1" badge="Compatibility Update" date="2026-03-29">
      **Release Scope**

      - Compatibility Update standardizing the MikroTik-related executor name.

      **Changes**

      - Renamed the MikroTik-related executor to `ros_address_list` to better match its actual behavior and naming style.
      - Corrected documentation examples and feature descriptions.
      - Applied formatting cleanup to keep code and docs aligned.

      **Compatibility and Upgrade Notes**

      - Deployments that used the old MikroTik executor name in `v0.1.0` need to update the plugin type when upgrading to `v0.1.1`.
  </ReleaseCard>

  <ReleaseCard version="v0.1.0" badge="First Public Release" date="2026-03-28">
      **Release Scope**

      - First Public Release providing the initial OxiDNS plugin architecture and core capability set.

      **Changes**

      - Established the OxiDNS plugin architecture around `server -> DnsContext -> matcher / executor / provider -> upstream or side effects`.
      - Completed server and upstream support for UDP, TCP, DoT, DoQ, and DoH.
      - Delivered MosDNS-style `sequence` orchestration, `jump/goto/return` control flow, and `$tag` references.
      - Added core executors such as `cache`, `forward`, `fallback`, `hosts`, `redirect`, `ecs_handler`, and `dual_selector`.
      - Added `domain_set`, `ip_set`, query / response predicates, client IP, response IP, and CNAME-based matching capabilities.
      - Added the management API, health checks, control endpoints, and plugin-related API surfaces. Added CLI commands with service-manager integration.
      - Added Debian packaging, Docker workflow support, and the initial multi-platform release pipeline.
      - Built reusable upstream connection pools and fetchers for UDP, TCP, DoT, DoH, and DoQ, and optimized matchers, cache, pools, request mapping, and clock updates on the hot path.

      **Compatibility and Upgrade Notes**

      - Tokio worker-thread count is configurable from runtime config.
      - MikroTik RouterOS dynamic-route and address-list integration is available.
      - Linux `ipset` / `nftset` command integration and test coverage are available.
      - The first round of Chinese and English README, quickstart, configuration, and module documentation is included.
  </ReleaseCard>
</div>
