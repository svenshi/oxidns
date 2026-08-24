# Operations Runbook

This runbook is for maintaining a deployed OxiDNS instance. User-facing command
and API references remain under `docs/docs/`; this document defines the order
of checks, safe change procedures, and recovery expectations for maintainers.

## Know the Runtime Contract

Record these values for every deployment:

- Binary version and bundle.
- Installation method: standalone archive, Debian package, service manager, or
  container.
- Absolute config path and working directory.
- DNS listener addresses and protocols.
- Management API address, TLS/auth mode, and WebUI root.
- Persistence, cache dump, provider data, log, upgrade cache, and backup paths.
- Upstream endpoints, outbound profiles, proxies, and bootstrap resolvers.
- External side effects such as ipset, nftset, or RouterOS ownership prefixes.

Relative runtime paths resolve from `-d/--working-dir`. Treat that working
directory as part of the deployment contract, especially for services and
upgrades.

## Preflight and Identity

Before starting or reloading:

```bash
oxidns --version
oxidns build-info
oxidns check -c /etc/oxidns/config.yaml -d /var/lib/oxidns --graph
```

Adjust paths for the installation. `build-info` is the source of truth for the
compiled bundle, features, and available plugin types. A config may be valid in
the repository but unsupported by a slim deployed binary.

For foreground diagnosis:

```bash
oxidns start -c /etc/oxidns/config.yaml -d /var/lib/oxidns -l debug
```

Do not run a second foreground instance on production listener ports while the
service is active.

## Service Operations

The built-in service commands are:

```bash
sudo oxidns service install -d /var/lib/oxidns -c /etc/oxidns/config.yaml
sudo oxidns service start
sudo oxidns service stop
sudo oxidns service restart
sudo oxidns service uninstall
```

Installation registers autostart but does not start the service. The working
directory must be absolute. The generated service uses restart-on-failure with
a short delay; repeated restarts indicate a persistent startup problem and
must not be treated as recovery.

Linux service discovery supports systemd, OpenRC, and OpenWrt/ImmortalWrt
procd. On procd systems the built-in installer owns `/etc/init.d/oxidns` and
uses the normal `enable`, `start`, `stop`, and `restart` actions.

The packaged Linux unit starts:

```text
/usr/bin/oxidns start -c /etc/oxidns/config.yaml -d /var/lib/oxidns
```

When diagnosing a service, compare its actual command line with the intended
config and working directory before investigating plugin behavior.

## Health and Readiness

With the management API enabled, probe the configured API base:

```bash
curl -fsS http://127.0.0.1:9199/api/healthz
curl -fsS http://127.0.0.1:9199/api/readyz
curl -fsS http://127.0.0.1:9199/api/health
curl -fsS http://127.0.0.1:9199/api/build
```

Use HTTPS and configured authentication in protected deployments. Avoid
placing long-lived credentials directly in shared shell history.

Endpoint meanings:

- `/api/healthz`: the management API listener is up. It does not prove DNS
  plugins are ready.
- `/api/readyz`: plugin initialization completed and at least one server plugin
  started.
- `/api/health`: detailed state, version, bundle, uptime, instance ID, and
  plugin/server counts. It returns a status document even when DNS is not
  ready; inspect the JSON fields.
- `/api/build`: compiled features and supported plugin types.

For orchestration, use `healthz` as liveness and `readyz` as readiness. If the
binary is built without the `api` feature, use DNS probes and service/process
state instead.

## Safe Configuration Change

1. Preserve the current config and note its version/hash.
2. Edit a candidate outside the live file when possible.
3. Run `oxidns check` with the same working directory as the service.
4. Review the dependency graph for missing, wrong-kind, or circular plugin
   references.
5. Replace or save the config atomically.
6. Request reload through `POST /api/reload` or restart the service when reload
   is not available.
7. Poll `GET /api/reload/status`, then verify readiness and perform DNS probes.
8. Keep the previous config until the observation window completes.

The config API supports validation and version-aware saves. API clients should
use the returned version instead of blindly overwriting concurrent edits.
Overlapping reload requests are rejected; wait for the active reload to finish.

If replacement runtime initialization fails, diagnose the reported plugin and
dependency error before retrying. Do not loop reload requests.

## Observability

### Logs

- Use configured file/stdout logging for startup and fatal errors.
- The API exposes recent log entries and an SSE stream when enabled.
- Increase log level only for a bounded diagnostic window; debug/trace logging
  can materially change hot-path results.
- Correlate incidents using instance ID, start time, plugin tag, protocol, and
  upstream tag where available.

### Metrics

Prometheus text is served at `/api/metrics` when the `metrics` feature is
enabled. Start with:

- `server_request_total`, completion/failure counters, inflight, and latency
  sum/count.
- `query_total`, `query_error_total`, inflight, and latency sum/count.
- Cache hit/miss/expired/skip/refresh counters and entry count.
- Forward success/error/timeout and per-upstream counters.
- Fallback, rate-limit, and local-answer counters.
- External integration queue, rejection, reconnect, degraded, and sync error
  metrics.

Metrics expose counters and sum/count pairs, not a complete latency histogram.
Calculate rates over time and compare with a known baseline. Labels must remain
low cardinality; use query recorder data rather than metrics for individual
domains or clients.

## Incident Triage Order

Use this order to avoid changing multiple layers at once:

1. Confirm process/service state and restart loop status.
2. Confirm version, bundle, config path, and working directory.
3. Check API liveness and readiness, then inspect `/api/health`.
4. Read startup/reload logs for the first causal error.
5. Probe the configured DNS listener locally over the affected protocol.
6. Probe upstream connectivity with `oxidns probe upstream`, using the deployed
   config/outbound profile when required.
7. Inspect metrics for saturation, timeouts, cache behavior, or external
   integration degradation.
8. Compare against the last known-good config and binary.
9. Recover one layer at a time and record the result.

## Common Failure Modes

### API is live but DNS is not ready

- Inspect `checks.plugin_init`, `checks.server_startup`, and server plugin count.
- Verify at least one server plugin exists and its entry executor resolves.
- Check listener address conflicts, permissions, TLS files, and feature support.
- Do not use `/healthz` alone as DNS readiness.

### DNS listener responds but queries time out or fail

- Separate local/synthetic answers from upstream-dependent queries.
- Probe each upstream with the same outbound, bootstrap, proxy, and TLS policy.
- Inspect forward timeout/error counters and fallback activation.
- Check connection pool saturation, resolver errors, and network route changes.

### Latency increases

- Compare server/query inflight and latency sum/count rates.
- Separate cache hit, miss, and upstream paths.
- Check debug logging, query recording, scripts, HTTP side effects, and
  synchronous external integrations.
- Check cache maintenance/occupancy and upstream latency before changing worker
  counts or timeouts.
- Reproduce with the performance procedure in `ai/performance.md`.

### Cache behavior is unexpected

- Inspect hit/miss/expired/skip/entry metrics and cache plugin configuration.
- Confirm QTYPE/QCLASS, ECS keying, negative caching, and TTL policy.
- Treat flush/load API operations and persistence loading as operational events.
- Preserve a suspect dump before clearing it when diagnosing format or pruning
  issues.

### RouterOS, ipset, or nftset is degraded

- DNS responses should be assessed independently from observer side effects.
- Inspect queue drops/capacity rejection, reconnect, backoff, sync errors, and
  degraded metrics.
- Verify credentials, transport/TLS, ownership prefix, target table/list, and
  platform privileges.
- Avoid manual deletion of foreign entries; OxiDNS cleanup is ownership-aware.

## Upgrade and Rollback

Use the staged workflow when risk is material:

```bash
oxidns upgrade check
oxidns upgrade download
sudo oxidns upgrade apply --no-restart
```

The download path verifies the GitHub release asset SHA256 digest. Apply uses an
upgrade lock, creates a binary backup, and can back up/replace WebUI assets.
Default cache and backup directories are relative to the working directory
unless explicit paths are supplied.

Before applying:

- Validate the target bundle and platform asset.
- Preserve config and persistence data separately from the binary backup.
- Confirm free space and write permissions for binary, WebUI, cache, and backup
  directories.
- Decide whether automatic restart is acceptable.

After applying:

- Verify `--version`, `build-info`, readiness, one local query, one upstream
  query, and the WebUI/API when shipped.
- Confirm the service is not restart-looping.
- Keep backups until the observation window passes.

For rollback, stop the service, restore the known-good binary and matching
WebUI backup using the platform's normal file-management procedure, restore the
previous config if it changed, start the service, and repeat the same health and
DNS checks. Do not delete upgrade backups before a successful verification.

## Incident Record

Capture enough evidence for later maintenance:

- Start/end time and timezone.
- Version, bundle, platform, install method, and instance ID.
- Config version and recent config/release changes.
- User-visible symptom and affected protocols.
- Health snapshots, relevant metric rates, and the first causal logs.
- Commands/probes run and their results.
- Recovery action, rollback point, and follow-up tests or documentation needed.

Security incidents and vulnerability reports follow `SECURITY.md`; do not put
secrets, tokens, private DNS data, or credentials into public issue logs.
