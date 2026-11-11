---
title: Operations and Troubleshooting
---

This chapter is for operators of deployed OxiDNS instances. It defines a practical order for preflight checks, safe changes, health evaluation, and incident diagnosis. See [CLI Tools](cli.md) for complete command arguments and [Management API](api.mdx) for endpoint fields.

## Record the Deployment Baseline

Keep at least the following information for every instance:

| Item | Example |
| --- | --- |
| Version and bundle | `oxidns 1.5.1`, `full` |
| Installation method | release archive, Debian package, Docker, OpenWrt |
| Configuration path | `/etc/oxidns/config.yaml` |
| Working directory | `/var/lib/oxidns` |
| DNS listeners | UDP/TCP `:53`, plus any DoT/DoH/DoQ addresses |
| Management plane | API address, TLS/auth mode, WebUI root |
| Persistent data | cache dumps, SQLite, provider files, logs |
| External integrations | ipset, nftset, RouterOS targets and ownership prefixes |

Runtime-relative paths resolve from `-d/--working-dir`. The same configuration with a different working directory can point logs, rules, SQLite, WebUI, and upgrade data at different locations, so the working directory is part of the deployment contract.

## Preflight Checks

Before a start, reload, or upgrade, run:

```bash
oxidns --version
oxidns build-info
oxidns check -c /etc/oxidns/config.yaml -d /var/lib/oxidns --graph
```

Confirm that:

- `build-info` contains every protocol and plugin used by the configuration; slim bundles do not include every capability.
- The validation `-d` matches the service's real startup arguments.
- The dependency graph has the intended entry points and matcher/executor/provider references.
- Port 53 is not already owned by systemd-resolved, dnsmasq, AdGuard Home, or another DNS service.
- The service account can access TLS files, rules, and persistence directories.

For foreground diagnosis, stop the existing service first and run:

```bash
oxidns start -c /etc/oxidns/config.yaml -d /var/lib/oxidns -l debug
```

Do not run a second foreground instance on production listener ports.

## Interpreting Health Endpoints

With the management API enabled, probe:

```bash
curl -fsS http://127.0.0.1:9199/api/healthz
curl -fsS http://127.0.0.1:9199/api/readyz
curl -fsS http://127.0.0.1:9199/api/health
curl -fsS http://127.0.0.1:9199/api/build
```

Use HTTPS and the configured authentication in protected deployments. Avoid putting long-lived credentials directly into shared shell history.

| Endpoint | What it proves | What it does not prove |
| --- | --- | --- |
| `/api/healthz` | The management listener is up | DNS plugins are ready |
| `/api/readyz` | Plugin initialization completed and a server started | Every external upstream is healthy |
| `/api/health` | Version, bundle, instance, plugin, and startup state | A real DNS query will succeed |
| `/api/build` | Capabilities compiled into this binary | The current configuration was applied correctly |

Use `healthz` as liveness and `readyz` as readiness. Builds without the API feature require process state plus real DNS probes.

## Safe Configuration Changes

Use this sequence:

1. Back up the active config and related provider/persistence files; record the current version or hash.
2. Edit a separate candidate file when possible instead of overwriting the live file directly.
3. Run `oxidns check` with the service's real working directory.
4. Review `--graph` for unintended dependency or entry changes.
5. Replace the config atomically, or save it through the version-aware config API.
6. Request reload; do not loop retries while another reload is active.
7. Wait for `/api/reload/status`, then verify readiness, DNS behavior, and key metrics.
8. Keep the previous config through the observation window.

Prefer provider-level reload when only rule data changed. Use application reload for plugin topology, global settings, or server changes. A failed application reload attempts to restore the old runtime, but the rebuild window can still cause a brief interruption.

## Incident Triage Order

Check one layer at a time:

1. Confirm process/service state and any restart loop.
2. Confirm version, bundle, config path, and working directory.
3. Check API liveness, readiness, and `/api/health`.
4. Find the first causal startup/reload error in logs.
5. Query the real DNS listener locally.
6. Validate affected upstreams with `oxidns probe upstream`.
7. Inspect inflight, timeout, cache, forward, and integration metrics.
8. Compare with the last known-good config, binary, and baseline.
9. Recover one layer at a time and repeat the probes.

### API Is Live but DNS Is Not Ready

- Confirm at least one server plugin exists and its `entry` resolves to an initializable executor.
- Check address conflicts, low-port privileges, and TLS file permissions.
- Use `/api/build` to confirm that the bundle includes the required server protocol.
- Do not treat `/healthz` alone as DNS readiness.

### The Listener Responds but Queries Fail

- Test local/synthetic answers separately from upstream-dependent names.
- Probe every upstream with the same outbound, bootstrap, SOCKS5, and TLS policy.
- Inspect forward timeout/error, fallback activation, and resolver errors.
- Check for bootstrap or forwarding loops caused by making OxiDNS its own default resolver.

### Latency Increases Suddenly

- Separate cache-hit, cache-miss, and upstream paths instead of relying on a global average.
- Inspect inflight, upstream latency, connection pools, timeouts, and queue drops.
- Check whether debug/trace, query recording, scripts, HTTP callbacks, or synchronous integrations were recently enabled.
- Do not raise worker counts or timeouts without evidence; that can hide queueing rather than fix it.

### Cache Behavior Is Unexpected

- Inspect hit, miss, expired, skip, lazy-refresh, and entry-count metrics.
- Confirm QTYPE/QCLASS, ECS, negative-cache, and TTL settings.
- Preserve the incident state before a flush or dump import; record those actions as operational events.

### ipset, nftset, or RouterOS Is Degraded

- Evaluate the primary DNS response separately from observer side effects.
- Inspect queue drops, reconnects, backoff, sync errors, and degraded metrics.
- Verify privileges, credentials, TLS, ownership prefixes, and target sets/tables.
- Do not bulk-delete entries outside the OxiDNS ownership namespace.

## Upgrade and Rollback

For higher-risk environments, stage the upgrade:

```bash
oxidns upgrade check
oxidns upgrade download
sudo oxidns upgrade apply --no-restart
```

Before apply, confirm platform, bundle, free space, WebUI path, and permissions. Back up configuration and persistent data separately; the binary backup created by the upgrader does not replace backups of config, SQLite, or provider data.

After replacement, explicitly start or restart the service using the installation method, then continue with the checks below.

After apply, verify in order:

1. `oxidns --version` and `oxidns build-info`.
2. The service is not restart-looping.
3. `readyz` succeeds.
4. One local/synthetic query and one real upstream query.
5. WebUI, management API, logs, and key metrics.

For rollback, stop the service, restore the matching known-good binary, WebUI, and config, then repeat the same health and DNS checks. Keep upgrade backups until verification succeeds.

## Evidence to Keep Before Reporting an Issue

Prepare:

- OxiDNS version, bundle, platform, and installation method.
- A redacted minimal config and working-directory argument.
- Affected protocol, query example, and expected result.
- The first causal error rather than only the final retry message.
- Health/build snapshots and relevant metric changes.
- Commands or probes already run and their results.

Do not publish passwords, tokens, TLS private keys, private names, client addresses, or complete query history. Report security issues privately using the channels in [Security Hardening and Vulnerability Reporting](security.md).
