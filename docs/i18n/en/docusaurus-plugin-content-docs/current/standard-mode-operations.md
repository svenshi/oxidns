---
title: Standard Mode Explanation, Diagnostics, and Assets
---

Standard Mode connects intent, generated native YAML, and individual queries with a stable revision. All product explanation happens in the WebUI; the OxiDNS backend handles only native configuration, generic transactions, and raw runtime events. Standard Mode does not modify OpenWrt/UCI, OS DNS, DHCP, firewalls, or third-party controllers.

## Browser compilation and explanation

The TypeScript compiler migrates and normalizes schema 1–6 intents, then produces:

- an SHA-256 `intentRevision` over normalized intent;
- stable object-ID mappings to Provider, Matcher, Path, Cache, Upstream, and Listener tags;
- final priority, path boundaries, capability diagnostics, dependency graph, and semantic Diff;
- deterministic native OxiDNS YAML.

The compiler obtains current build capabilities from `/api/build`. Browser checks provide product feedback, while generic `/api/config/validate` performs the final validation of native YAML, includes, and relative paths. Apply accepts only YAML protected by version CAS, never an Intent or Plan.

## Bounded query traces and client explanation

The Standard-generated `query_recorder` may write `intentRevision` and generated context as an ordinary string map. Each query defaults to 512 events; Expert YAML accepts 32–4096. On overflow, the backend returns `steps_truncated` and `dropped_step_count`.

The backend returns only raw Cache, Forward, and Fallback steps, timing, and truncation facts. It neither generates `diagnosis` nor interprets revisions. The WebUI adds rule, path, cache, and upstream explanations only when the recorded revision matches the active Intent and generated mapping. A mismatch falls back to raw facts, avoiding guesses about old records from the current workspace.

Existing query-recorder SQLite databases continue to load through incremental migration. Events remain bounded by per-request limits, upstream fan-out, recorder queues, retention, and database read concurrency, and must never contain upstream credentials.

## Assets and recovery

- Intent import/export is local to the browser. Imported data still goes through compilation, Diff, generic validation, and Apply.
- Templates live in browser IndexedDB, scoped by OxiDNS instance, with at most 64 records and 2 MiB total.
- Expert Copy only places generated YAML in the Expert editor; it changes neither disk nor runtime.
- Expert Analysis calls generic `/config/validate`; product classification comes from the frontend plugin catalog.
- The active Intent, generated mappings, and latest generation metadata are opaque JSON in `/api/webui/config`. After DNS activation, workspace CAS retries at most three times. Failure does not roll back DNS and the UI offers an Intent download.
- Backend healthy-YAML history retains at most 20 versions and 20 MiB. Restore returns only a YAML preview that must pass Validate and Apply again.
- Removing dynamic-learning configuration does not delete old rule or metadata files automatically. The WebUI reports leftovers; the backend exposes no mode-specific deletion API.

See [Standard Mode and Backend Boundary](api/standard-mode.mdx) for the workspace boundary and [Configuration API](api/configuration.mdx) for transaction fields.
