# Request-path profiling

The `Hotpath` workflow runs the request-time cases listed in
[the benchmark catalog](../.github/hotpath-benchmarks.json). Each case runs in
its own process, first natively and then under Callgrind. Reports combine native
timing/allocation data with instruction counts and the policy in
[policy.toml](policy.toml). Jobs are grouped by Cargo benchmark target to reuse
compilation. The catalog fixes iteration counts per target; async plugin and
pipeline cases use fewer iterations to bound job duration. Ruleset construction
is initialization work and is excluded.

The existing codec Cloud series retain their names so history stays continuous.
To add a case, use the hotpath Criterion adapter in its benchmark, list the exact
Criterion ID and a unique Cloud series name in the catalog, and verify the native
report. Keep fixtures deterministic and local: external upstreams, firewall or
RouterOS side effects, filesystem persistence, and wall-clock expiry/probing are
unsuitable for instruction-count comparisons. Request-pipeline fixtures warm
cache entries before measurement and verify that hits bypass downstream work.

`just bench-hotpath <criterion-id> [cargo-benchmark-target]` runs a single case;
the target defaults to `message_codec`. Set `HOTPATH_CRITERION_ITERATIONS` to a
positive fixed count to produce a report. The workflow uses `jq` to generate its
matrix directly from the catalog and a shell loop to run both measurements for
each case. `HOTPATH_PROFILE` can select a faster local build profile for smoke
checks; local timings are diagnostic only.

Pull requests upload artifacts without OIDC permissions. The `Hotpath relay`
workflow reads the catalog from its trusted default-branch revision in a separate
job with read-only permissions. Each report is then passed to the author's
[hotpath-relay Action](https://github.com/hotpath-rs/hotpath-relay) at the immutable
`v1.0.0` release. It downloads the report, identifies the PR from GitHub, stamps
metadata, and uploads to Cloud. The upload jobs run only that Action and never
check out or execute PR code. Concurrency groups include the individual series
so reports from the same run do not cancel one another.

The relay runs from the default branch only; renaming the `Hotpath` workflow
requires updating the relay's trigger, and the relay run is not a PR check.

When changing the runner or relay, validate both workflows with actionlint and
smoke-test catalog cases before handoff. Full Callgrind and OIDC upload validation require Linux and GitHub
Actions respectively; a local native run does not establish either.
