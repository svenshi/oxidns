# Operations Runbook

Commands below illustrate the operational sequence. Resolve current CLI flags
from `src/cli/` or `oxidns --help`, service behavior from
`src/infra/service.rs`, API routes from `src/api/` and the generated API docs,
and configuration/defaults from `src/config/` plus `config*.yaml`. Those project
surfaces override copied examples in this runbook.

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

Before starting or reloading, use the current CLI commands to record binary
identity/build capabilities and validate the deployed config with its real
working directory and dependency graph. `src/cli/`, `oxidns --help`, and the CLI
user documentation define the exact syntax. A config may be valid in the
repository but unsupported by the deployed binary's compiled capabilities.

For foreground diagnosis, use the start command and log-level options exposed
by the installed binary, substituting the deployment's actual config and
working-directory paths.

Do not run a second foreground instance on production listener ports while the
service is active.

## Service Operations

Use the service operations exposed by `oxidns service --help`. Their supported
actions and generated definitions come from `src/infra/service.rs`; packaged
unit files and installation scripts define distribution-specific behavior.
Inspect the installed definition rather than relying on copied defaults.
Repeated restarts indicate a persistent startup problem and must not be treated
as recovery.

When diagnosing a service, compare its actual command line with the intended
config and working directory before investigating plugin behavior.

## Health and Readiness

With the management API enabled, probe the configured API base using the
liveness, readiness, detailed health, and build-capability routes registered by
`src/api/` and documented under `docs/docs/api/`.

Use HTTPS and configured authentication in protected deployments. Avoid
placing long-lived credentials directly in shared shell history.

Do not confuse API liveness with DNS readiness. Use the route semantics and
response fields implemented by the current handlers for orchestration. If the
deployed binary has no management API, use DNS probes and service/process state
instead.

## Safe Configuration Change

1. Preserve the current config and note its version/hash.
2. Edit a candidate outside the live file when possible.
3. Run `oxidns check` with the same working directory as the service.
4. Review the dependency graph for missing, wrong-kind, or circular plugin
   references.
5. Replace or save the config atomically.
6. Request reload through the currently documented control route, or restart
   the service when reload is unavailable.
7. Follow the operation/status contract exposed by the API, then verify
   readiness and perform DNS probes.
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

The metrics route and current metric catalog are documented in
`docs/docs/api/metrics.mdx` and implemented by metric-source registration in
Rust. Start with request/error/inflight/latency signals, then inspect the owning
cache, upstream, policy, or side-effect subsystem. Calculate rates over time and
compare with a known baseline. Labels must remain low cardinality; use record or
query APIs rather than metrics for individual domains or clients.

## Incident Triage Order

Use this order to avoid changing multiple layers at once:

1. Confirm process/service state and restart loop status.
2. Confirm version, bundle, config path, and working directory.
3. Check API liveness and DNS readiness, then inspect detailed health state.
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

- Inspect the current readiness diagnostics and server/plugin state returned by
  the health handler.
- Verify at least one server plugin exists and its entry executor resolves.
- Check listener address conflicts, permissions, TLS files, and feature support.
- Do not use API liveness alone as DNS readiness.

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

Use the staged check, download, and apply operations exposed by the installed
CLI when risk is material. Exact flags and current verification/backup behavior
come from `src/cli/`, `src/infra/upgrade/`, CLI documentation, and upgrade tests;
review them before production use.

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
