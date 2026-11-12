# Request-path profiling

The [Hotpath workflow](../.github/workflows/hotpath.yml) runs benchmarks and
publishes Markdown tables in GitHub Actions Summary. Full native and Callgrind
reports are saved as artifacts on pushes and pull requests. No hotpath Cloud
account, OIDC permission, relay workflow, or external reporting service is used.

## Selection

[The catalog](../.github/hotpath-benchmarks.json) is the executable selection:

- `core` runs on pushes and PRs. It covers codec size/compression differences,
  the domain matching engines, fragmented IP lookups, plugin lookup and rewrite
  paths, cache hits/admission, request handling, and response classification.
- `all` adds diagnostic variants, small/large ruleset construction, and a local
  UDP forwarding fixture. It runs weekly and can be selected manually.

The full tier includes core cases. Build cases override the target iteration
budget so loading large rule sets does not repeat thousands of times. Local UDP
forwarding is explicitly native-only: every query reaches a deterministic
loopback authority through the real forward executor, but socket scheduling is
unsuitable for instruction comparisons. Existing DNS load and concurrency
scenarios remain in [`benchmarks/`](../benchmarks/README.md).

Keep a case when it exercises a distinct algorithm, request-path cost, workload
size, or resource-pressure behavior. Do not multiply the same address-family and
record-count matrix across every response-producing plugin. Prefer ordinary and
large compressed codec cases in core; compatibility and less common type
variants remain available for diagnosis. Startup performance belongs in the
extended tier, not a request-cost aggregate.

## Measurement contract

Each deterministic case runs in its own process, first natively and then under
Callgrind, using the same build, exact Criterion ID, and iteration count. The
repository-owned runner validates both reports and packages them together; it
does not enable `hotpath-cloud` or use hotpath's cloud-dependent merge API.

The summary reads the `hotpath::criterion::routine` row, not the enclosing bench
row. This excludes batched setup and teardown. Cache admission uses a fresh,
bounded cache for every iteration: warm-cache hits cannot silently replace the
intended miss/insert path, and growing key streams do not introduce timed
pruning. A probe verifies that the first request inserts and the second hits.
Warm-cache probes verify that downstream work is bypassed. Negative cache
fixtures include SOA and disable the no-SOA fallback.

IP fixtures contain gaps so compilation preserves fragmented ranges instead of
collapsing thousands of adjacent prefixes into one interval. IPv4 hits and
misses probe the same populated bitmap page; IPv6 probes exercise multi-range
search. Compact inline/linear lookups remain in the diagnostic tier. Domain
suffix cases distinguish hits, shared-suffix misses, and fast unrelated misses;
extra unmatched left labels are not treated as deeper trie traversal.

Plugin microbenchmarks clone prebuilt queries rather than parsing names on every
iteration. Registry teardown is explicit. The complete redirect fixture verifies
the restored question and inserted CNAME after a downstream answer. Request
handling includes ingress/finalization and EDNS, with server metrics disabled.

Allocation mode is cumulative, so the routine includes nested function
allocations without summing and double-counting their rows. Current hotpath
non-cloud JSON rounds display units; normalized allocation/timing values are
therefore approximate. Instruction totals are integer counts normalized by the
validated iteration count. Reports retain source revision, fixture and lockfile
hashes, feature selection, profile, compiler, platform, and profiler versions.

## Baselines and interpretation

The workflow tries to download the last successful push run on the repository's
default branch. Missing/expired artifacts or unavailable read permissions do not
prevent measurement. The first run after migration establishes the baseline;
old Cloud artifacts are not compared.

Instruction and allocation changes are shown only when fixture identity,
iterations, profile, features, compiler, architecture, hotpath version, and
Callgrind version match. A changed fixture starts a new comparison baseline.
Native-only cases show their current timing/allocation values without an
instruction delta. Raw reports remain inspectable when comparison is skipped.

Changes are informational, not automatic failure thresholds. Native timing
includes instrumentation and shared-runner noise. Do not treat these numbers as
release performance claims, and do not compare Callgrind elapsed time with native
time. Use the load-test pack separately for throughput, tail latency, and
concurrency scaling.

## Local use and validation

```bash
# Print the exact CI matrix and validate the catalog.
python3 .github/scripts/hotpath.py catalog --tier core

# Run one target with both native and Callgrind measurement (Linux).
python3 .github/scripts/hotpath.py run \
  --target request_pipeline --output /tmp/oxidns-reports

# Fast native smoke check on a developer machine.
HOTPATH_PROFILE=dev python3 .github/scripts/hotpath.py run \
  --target request_pipeline --tier all --iterations 2 --native-only \
  --output /tmp/oxidns-smoke

# Render results, optionally against a downloaded/extracted baseline artifact.
python3 .github/scripts/hotpath.py summary /tmp/oxidns-reports \
  --baseline /tmp/oxidns-baseline

# Validate identity checks, metric normalization, and comparison rules.
python3 -m unittest discover -s .github/scripts -p 'hotpath_test.py'
```

`just bench-hotpath <criterion-id> [cargo-benchmark-target]` remains the primitive
for a single case. Its recipe defines the build features and accepts
`HOTPATH_PROFILE`; fixed-iteration reporting requires a positive
`HOTPATH_CRITERION_ITERATIONS`. Without that variable the benchmark binaries use
normal Criterion sampling. Cloud upload is never part of the recipe.

When changing fixtures or the catalog, smoke-test every catalog ID and its
assertions. Validate workflow edits with actionlint and run the applicable gates
from `justfile`. Full instruction-report validation requires Linux and Valgrind;
a macOS native smoke check does not establish that path or GitHub artifact access.
