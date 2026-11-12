# Request-path profiling

The [Hotpath workflow](../.github/workflows/hotpath.yml) runs benchmarks and
publishes Markdown tables in GitHub Actions Summary. Full native and Callgrind
reports are saved as artifacts on pushes and pull requests. No hotpath Cloud
account, OIDC permission, or external reporting service is used. A separate
comment workflow updates one PR comment after measurement, including fork PRs.

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

Regression thresholds are hardcoded in `policy()` in
[the report script](../.github/scripts/hotpath.py); no policy file is needed. It gates per-operation instruction and cumulative allocation
increases. Both a relative percentage and an absolute floor must be exceeded;
equality is within the limit. A zero baseline is judged by the absolute floor.
The same thresholds distinguish significant improvements. Current defaults are
3% for instructions, and 20% plus 16 B/op for allocations. Review threshold
changes in the script like workflow changes. Timing is diagnostic only and
never fails the gate.

The visual report follows the structure of the [official hotpath PR example](https://github.com/hotpath-rs/hotpath/pull/764#issuecomment-6430183334):
a verdict and counts first, baseline → head values with percentages, and folded
related metrics. 🔴 ↑ means a regression beyond policy; 🟢 ↓ is a significant
improvement; ⚪ shows changes within limits; ⚠️ means no valid comparison.
Regressions appear first. One regressed metric fails a case even if another
metric improves. An empty report set fails; missing or incompatible baselines
are explicitly skipped so the initial baseline can be established.

`Performance gate` is the stable aggregate PR check. Any benchmark, report
validation, or threshold failure makes it fail, while JSON artifacts and the
Summary are preserved. PR workflows run even for docs-only changes so a required
check does not remain pending due to path filters. To block merges, make
`Performance gate` required in repository branch protection/rulesets; committing
a workflow alone does not change those repository settings.

[`hotpath-comment.yml`](../.github/workflows/hotpath-comment.yml) triggers on
completion, including failed runs, and checks out only its trusted default-branch
revision. The publisher resolves the PR through GitHub's API, checks the current
head and newest run/attempt, and creates or updates a single bot-owned comment.
It reads bounded JSON members from ZIP artifacts without extracting or executing
them, validates metric values, and renders its own Markdown. Download redirects
drop credentials when crossing hosts. It does not use `pull_request_target` or
execute PR code with write permissions. The comment workflow must land on the
default branch before GitHub can trigger it. Local tests mock all comment writes.

Do not treat these numbers as release performance claims or compare Callgrind
elapsed time with native time. Use the load-test pack separately for throughput,
tail latency, and concurrency scaling.

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

# Evaluate policy and save a machine-readable result (exit 1 on regression).
python3 .github/scripts/hotpath.py gate /tmp/oxidns-reports \
  --baseline /tmp/oxidns-baseline --output /tmp/oxidns-gate/result.json

# Validate metrics, thresholds, PR association, and safe comment updates.
python3 -m unittest discover -s .github/scripts -p 'hotpath*_test.py'
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
