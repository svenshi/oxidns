---
title: Performance Benchmark
sidebar_position: 8
---

# Performance Benchmark

This page presents a periodic benchmark snapshot of OxiDNS `oxidns 1.5.1 (full)`, mosdns `v5.3.4-0-gb732318`, AdGuard Home `AdGuard Home, version v0.107.78`, and SmartDNS `smartdns 1.2026.06.28-1614 (Release48.2)`, measured with dnsperf `2.15.1`. It is updated for meaningful architecture, request-path, methodology, or milestone changes—not for every release.

Generated: `2026-07-24T19:07:42.209737+08:00`

## Method

- repeats per point: `3`; measured duration: `12s`; warmup: `3s`
- outstanding-query levels: `1,4,16,64,256,1024`
- aggregation: median across repeats; stable capacity excludes points with loss above 0.1%
- server CPU is aggregate process CPU (100% = one fully occupied logical CPU); memory is sampled RSS
- engines run one at a time and their order alternates by repeat/load point

## Environment

<details>
<summary>Recorded environment and complete SHA-256 input manifest</summary>

- `timestamp=2026-07-24T19:07:42.209737+08:00`
- `hostname=oxidns`
- `kernel=Linux-7.0.14-4-pve-x86_64-with-glibc2.36`
- `cpu=Model name:                              Intel(R) N100`
- `logical_cpus=4`
- `memory=Mem:           512Mi        59Mi        49Mi        80Ki       403Mi       452Mi`
- `git_head=n/a (release artifact benchmark)`
- `git_describe=n/a (release artifact benchmark)`
- `dnsperf_version=2.15.1`
- `benchmark_inputs_sha256={'.generated/common-domain-hit-answers.zone': '1067bb6a22bef6ce52b536eaa874951c7863e2e0abf47483b044dd0367553afc', '.generated/common-domains.txt': 'd9ea970afa80a6d9cebdda2fac825316f5108754a38f1016eeaccd41cd7060b5', 'configs/common/adguardhome/02-cache-hotpath.yaml': '5929474f03bee43b3ab512a9e03a2b6cb4ba24890635ab377498d8e6717d63d6', 'configs/common/adguardhome/47-server-local.yaml': '3b7a834ec2b8fc1295134ffbd069882b37ad730ff96ca3bbe9d9fcef514af50a', 'configs/common/adguardhome/48-server-local-tcp.yaml': '5d9652ac3d27459dccca3d3f0f555379b459998cba4debda8b63191cfdc40779', 'configs/common/adguardhome/50-common-local-answers.yaml': '8a2e1a39ba34adcbf3cebe28d73807eca9c84ca277d37485439325d764fc3e36', 'configs/common/adguardhome/51-common-domain-set.yaml': '6ea73cd6f207fbbf4dd14e6570ccd153e91b02b1145b2d9c80ed3197ad59a428', 'configs/common/adguardhome/52-negative-cache-hotpath.yaml': '039b5b5305edc928ff1d9d6f8005e5b3b083a56c07cc595819c51ce06e31f9fb', 'configs/common/mosdns/02-cache-hotpath.yaml': '09381cc7025304059179a04a6d18347e1306a5d5602cfdf02fbb1b0a973e6804', 'configs/common/mosdns/47-server-local-udp.yaml': '4aa53534aa2e98891a772f9309b496ed95cb56dc1722caef5bdf47e26eddb032', 'configs/common/mosdns/48-server-local-tcp.yaml': 'e1b8426a7c1b6c8f16b6d8b95e5f726bbe10bdaab9f20f8c2bba3f8473330df3', 'configs/common/mosdns/50-common-local-answers.yaml': '2ab4d4da99b8edac3dd543a109fd9c3721c2d514bd16b868aad1c35f54e45557', 'configs/common/mosdns/51-common-domain-set.yaml': 'ce20a88373d3c3291339c6834cab7bfa2dfc3ae4343cd4e55541e4f42f0c8d99', 'configs/common/mosdns/52-negative-cache-hotpath.yaml': '806cae46f6e82d7b501ce424be1ba92e25217774593adf98b95ae4de3dbe26cc', 'configs/common/oxidns/02-cache-hotpath.yaml': 'a6a0d37ea28f6c369c6be78e16790804fa21e019fe44c45e49f3c4e24bb7f3e6', 'configs/common/oxidns/47-server-local-udp.yaml': '65df077af27a44005428c652074c0048eadbe3f5238526fa26fde5b3aefcf0cf', 'configs/common/oxidns/48-server-local-tcp.yaml': 'cb23c83c69cacf4bf13871225c85d357a573e4b5f2a4afe0eecdf49689637ab8', 'configs/common/oxidns/50-common-local-answers.yaml': '3cd04d8951ecf069de27b68832d4e917e9e199e9713a363a88d21165fb2b0e09', 'configs/common/oxidns/51-common-domain-set.yaml': '6a6c6211adca409cee9978dc620f8f9753ed28fd8be7340fa5a7f3310c1c7970', 'configs/common/oxidns/52-negative-cache-hotpath.yaml': '23b493eae974ca42103738718121b76f235ce2288724133d03aba21fec70eea1', 'configs/common/smartdns/02-cache-hotpath.conf': '2419a63a482957cc322eb4a716fc14844ff399a21a14fc56de88113f3e5415ba', 'configs/common/smartdns/47-server-local-udp.conf': '833a5f47af78b43d6a9d5a206fdb082698a1f8cf2c1451d0fd421b4fd3176cb0', 'configs/common/smartdns/48-server-local-tcp.conf': 'c8c8939cca1a96f30133a7bc1e7bf67a2449c0170811c634dcda6b9ceb286e67', 'configs/common/smartdns/50-common-local-answers.conf': '4d4d96296cef6ef764f11d1016b109e6ab00c6d61a86671ea04335df7e2c66bc', 'configs/common/smartdns/51-common-domain-set.conf': 'd97589e230b937e9ca31926920714ab1c7f34c20d96015b7c6308e25356704e4', 'configs/common/smartdns/52-negative-cache-hotpath.conf': '2bc469d27e27fdddb0f9de8059e2f29693dbd0d87e5258ad6b3a705d4a94b974', 'data/geosite_cn.txt': '283413ec07896a187eebec5ab0c7eb6783899c40c9bf99b0b767218530d507f1', 'data/geosite_geolocation-!cn.txt': 'f2053f8f3420228213f5dbac2993f806eac8251dff810c81a9331b7627764e74', 'data/ip_set_rules.txt': '5ede49ff650a4fabdf23ed3d836b93bbaa7f69d839d5d0901c6975b78167dfd5', 'queries/cache-hotpath.txt': 'a4b3666a897facb65e823ce8193572d8811ea6abe477afdf636dfe49683d1d48', 'queries/cache-negative.txt': '9399aaaebebad2352f51dc7549973e4a1c76548f7b84a225a3afe46c0e08c3b2', 'queries/domain-set.txt': 'd7df7b58052c2b016c589d9ce4d5f887fe4bb0a831034fd61e99658af2c08248', 'queries/local-answers-common.txt': '89ace67a706a109e6331bd980f773dc9dfd2bd810330eb7ef961615a12de5b38', 'queries/plugin-a.txt': '225f834e548fd3af9d1dd6293df9d3de5713c0f33e3299434f9789a417ec31f5', 'run_publishable_compare.py': '1bc6978c8d9af98f0efbfe021f4be722151281c88fcb87ac23df79d5a8432d17', 'scenarios.tsv': '31431da0b8f62f16b2e7eab76324d233517fbd4bf5777ffc8da3055863ef37b0'}`
- `common_domain_corpus={"duplicate": 198, "included_full": 1247, "included_plain": 142119, "invalid_domain": 113, "normalized_unique_domains": 143366, "positive_query_domains": 24, "unsupported_regexp": 163}`
- `cache_upstream_fixture=deterministic UDP authority at 127.0.0.1:5453`
- `oxidns_version=oxidns 1.5.1 (full)`
- `oxidns_sha256=8cff1b81a6518f4436308750fb24700fd1389d747d171adaca809d1110e73518`
- `mosdns_version=v5.3.4-0-gb732318`
- `mosdns_sha256=5357fbb83c89f0a7acad275b72c33aa70d4c720cb5590525660132b10cee8af9`
- `adguardhome_version=AdGuard Home, version v0.107.78`
- `adguardhome_sha256=fad50bcebf485fa3e8eec3c01db2dded54d02dd73bdab18a8dc79db6ba99b655`
- `smartdns_version=smartdns 1.2026.06.28-1614 (Release48.2)`
- `smartdns_sha256=2e51d85a70ab30002c83a36fcc5e1a3e62169e0b561bbd1e7508419a21fdb33e`

</details>

## Semantic equivalence checks

- The common domain corpus is generated once and shared by all four engines. Supported `full:` prefixes are stripped; plain and `domain:` entries are retained; regex/keyword rules, invalid names, and duplicates are excluded because they cannot be mapped safely to every product.
- Normalized corpus statistics: `{"duplicate": 198, "included_full": 1247, "included_plain": 142119, "invalid_domain": 113, "normalized_unique_domains": 143366, "positive_query_domains": 24, "unsupported_regexp": 163}`.
- Before timing, every engine must return `192.0.2.53` for a corpus hit and `192.0.2.54` for the fixed miss control. The parsed DNS response evidence is saved in `semantic-validation.json`.
- Positive and negative caches are filled by the deterministic local authority. Every timed cache row must record zero upstream queries or publication is rejected.
- Response-IP/CIDR matching is excluded: the products expose different in-memory matching, response-filtering, and operating-system ipset semantics, so converting the input file would not create an equivalent workload.
- This run contains **432 real timed samples** and 144 three-run median points. The 24 scenario/engine groups passed **72 packet-level semantic probes**, and all 144 timed positive/negative-cache rows recorded zero upstream queries.
- The [environment snapshot](/benchmarks/staged/environment.json) stores SHA-256 hashes for all 36 effective inputs: runner, catalog, engine configs, queries, rule data, and generated normalized-domain assets.

## How to read the metrics

- **QPS / throughput: higher is better**, provided loss and tail latency remain acceptable.
- **p50/p95/p99/max latency: lower is better**. p99 is the response time that 99% of completed requests do not exceed; it is more useful than the average for spotting queueing and long-tail stalls.
- **Packet loss: lower is better**. This report only treats a point as stable when median loss is at most 0.1%.
- **CPU: lower is better at the same throughput**. CPU alone is not a speed score: higher CPU can be reasonable when it produces substantially more QPS. Here, 100% means one fully occupied logical CPU.
- **RSS memory: lower is better for the same workload**. RSS is the process's resident physical memory during the measured run.
- On scaling charts, the preferred curve rises with concurrency while latency and loss stay controlled. A flat QPS curve combined with rising p99 means the engine has reached saturation.

## Part I: Four-engine common matrix

This section compares only paths that all four products can express with equivalent input and response semantics. The separate OxiDNS/mosdns native-rule suite is preserved as Part II and is not merged into this ranking.

### Shared positive domain-match workload

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/domain-matching.svg" alt="Stable throughput and retained capacity with 143,366 domains" />
  </div>
  <div className="col col--4">
    <p><strong>OxiDNS: 122,941 QPS, about 103% retained capacity</strong></p>
    <p>With real positive matches across 143,366 domains, OxiDNS leads mosdns by 16.0%, SmartDNS by 54.1%, and AdGuard Home by 84.8%. The 103.3% ratio is run-to-run spread, so the objective conclusion is no measurable throughput loss; the main cost is about 17.7 MiB additional RSS.</p>
  </div>
</div>

### Charts

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/throughput.svg" alt="Maximum stable throughput by scenario" />
  </div>
  <div className="col col--4">
    <p><strong>Higher is better</strong></p>
    <p>OxiDNS has the highest maximum stable throughput in all five UDP scenarios; mosdns leads maximum completed TCP throughput. The TCP loss gate does not reveal deep queueing, so read the p99 chart next to it.</p>
  </div>
</div>

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/stable-tail-latency.svg" alt="p99 at each engine's maximum stable point" />
  </div>
  <div className="col col--4">
    <p><strong>Lower is better at the stated throughput</strong></p>
    <p>OxiDNS records 2.0–2.1 ms p99 at its UDP stable points. TCP q1024 has zero loss but 16.9–55.3 ms p99 across the engines, which is clear queueing rather than a recommended operating point.</p>
  </div>
</div>

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/scaling.svg" alt="Throughput scaling by concurrency" />
  </div>
  <div className="col col--4">
    <p><strong>Rising without flattening early is better</strong></p>
    <p>The UDP baseline shows OxiDNS and mosdns scaling through q256, while SmartDNS flattens around one logical CPU. All four q1024 points exceed 0.1% loss and are shown only to expose saturation.</p>
  </div>
</div>

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/tail-latency.svg" alt="p99 tail latency under load" />
  </div>
  <div className="col col--4">
    <p><strong>Lower is better</strong></p>
    <p>At UDP q256, p99 is 2.05 ms for OxiDNS, 2.43 ms for mosdns, 2.11 ms for SmartDNS, and 7.55 ms for AdGuard Home. q1024 exceeds the loss gate and is excluded from stable capacity.</p>
  </div>
</div>

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/cpu.svg" alt="CPU at maximum stable throughput" />
  </div>
  <div className="col col--4">
    <p><strong>Lower is better at equal throughput</strong></p>
    <p>Across the five UDP stable points, CPU per 10k QPS is about 17.1%–18.9% for OxiDNS, 14.3%–16.0% for mosdns, and 11.4%–13.5% for SmartDNS. OxiDNS trades more CPU for the highest capacity.</p>
  </div>
</div>

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/memory.svg" alt="Resident memory at maximum stable throughput" />
  </div>
  <div className="col col--4">
    <p><strong>Lower is better for the same workload</strong></p>
    <p>SmartDNS has the lowest stable-point RSS (6.0–22.2 MiB). OxiDNS uses 13.6–31.4 MiB versus mosdns at 22.8–41.9 MiB. AdGuard Home is highest in this matrix at 49.6–88.7 MiB.</p>
  </div>
</div>

### Maximum stable points in the four-engine matrix

| Scenario | Engine | Outstanding | QPS | p99 | CPU | RSS | Loss |
|---|---|---:|---:|---:|---:|---:|---:|
| 02-cache-hotpath | OxiDNS | 256 | 113,242.1 | 2.047 ms | 214.1% | 14.3 MiB | 0.0340% |
| 02-cache-hotpath | mosdns | 256 | 109,273.2 | 2.623 ms | 169.1% | 22.9 MiB | 0.0345% |
| 02-cache-hotpath | AdGuard Home | 256 | 65,555.6 | 7.935 ms | 269.0% | 51.8 MiB | 0.0290% |
| 02-cache-hotpath | SmartDNS | 64 | 78,353.9 | 0.927 ms | 99.8% | 6.3 MiB | 0.0000% |
| 47-server-local-udp | OxiDNS | 256 | 119,754.3 | 2.047 ms | 204.6% | 13.6 MiB | 0.0308% |
| 47-server-local-udp | mosdns | 256 | 111,019.1 | 2.431 ms | 159.3% | 22.8 MiB | 0.0303% |
| 47-server-local-udp | AdGuard Home | 256 | 72,285.7 | 7.551 ms | 258.7% | 50.0 MiB | 0.0250% |
| 47-server-local-udp | SmartDNS | 256 | 87,537.3 | 2.111 ms | 99.8% | 6.0 MiB | 0.0267% |
| 48-server-local-tcp | OxiDNS | 1024 | 151,822.6 | 16.895 ms | 249.5% | 14.7 MiB | 0.0000% |
| 48-server-local-tcp | mosdns | 1024 | 161,175.5 | 23.039 ms | 264.5% | 24.6 MiB | 0.0000% |
| 48-server-local-tcp | AdGuard Home | 1024 | 74,921.8 | 55.295 ms | 274.5% | 49.6 MiB | 0.0000% |
| 48-server-local-tcp | SmartDNS | 1024 | 91,752.7 | 20.479 ms | 99.8% | 6.3 MiB | 0.0000% |
| 50-common-local-answers | OxiDNS | 256 | 119,046.3 | 2.111 ms | 209.4% | 13.7 MiB | 0.0321% |
| 50-common-local-answers | mosdns | 256 | 111,199.5 | 2.431 ms | 159.7% | 22.9 MiB | 0.0341% |
| 50-common-local-answers | AdGuard Home | 256 | 70,829.2 | 7.679 ms | 256.5% | 50.5 MiB | 0.0254% |
| 50-common-local-answers | SmartDNS | 256 | 83,245.9 | 2.303 ms | 99.8% | 6.0 MiB | 0.0267% |
| 51-common-domain-set | OxiDNS | 256 | 122,941.0 | 2.015 ms | 209.6% | 31.4 MiB | 0.0311% |
| 51-common-domain-set | mosdns | 256 | 105,951.1 | 3.135 ms | 169.7% | 41.9 MiB | 0.0358% |
| 51-common-domain-set | AdGuard Home | 64 | 66,532.8 | 3.135 ms | 249.0% | 88.7 MiB | 0.0000% |
| 51-common-domain-set | SmartDNS | 64 | 79,781.2 | 0.911 ms | 99.8% | 22.2 MiB | 0.0000% |
| 52-negative-cache-hotpath | OxiDNS | 256 | 113,469.6 | 2.111 ms | 204.7% | 14.3 MiB | 0.0338% |
| 52-negative-cache-hotpath | mosdns | 256 | 107,298.8 | 2.687 ms | 169.0% | 22.8 MiB | 0.0351% |
| 52-negative-cache-hotpath | AdGuard Home | 256 | 63,017.5 | 8.703 ms | 267.9% | 52.2 MiB | 0.0277% |
| 52-negative-cache-hotpath | SmartDNS | 64 | 73,919.4 | 1.007 ms | 99.8% | 6.3 MiB | 0.0000% |

### Objective assessment of the four-engine matrix

- **OxiDNS's advantage is concentrated in local UDP capacity and domain matching, not every metric.** It has the highest maximum stable throughput in positive cache, UDP local response, A/AAAA local overrides, normalized domain matching, and negative cache. It leads mosdns by 3.6%–16.0%, SmartDNS by 36.8%–54.1%, and AdGuard Home by 65.7%–84.8%, while using more CPU per 10k QPS than mosdns in all five.
- **The 143,366-domain positive-match path is the clearest common-matrix OxiDNS result.** OxiDNS reaches 122,941.0 QPS at 2.015 ms p99 and 31.4 MiB RSS. Its 103.3% retained capacity against the 119,046.3-QPS local-answer baseline does not mean lookup makes it faster; it means no throughput penalty is measurable within three-run spread. The index adds about 17.7 MiB RSS. mosdns, SmartDNS, and AdGuard Home retain 95.3%, 95.8%, and 93.9%.
- **TCP is a genuine counter-result.** Under the shared loss≤0.1% definition, mosdns reaches 161,175.5 QPS at q1024, 6.2% above OxiDNS at 151,822.6. OxiDNS has 26.7% lower p99 (16.895 vs 23.039 ms) and 40.2% lower RSS. q1024 adds only about 13%–15% throughput over q256 while raising their p99 from 4.863/5.503 ms to 16.895/23.039 ms; q1024 QPS coefficients of variation are 19.8% for mosdns, 8.0% for OxiDNS, and 28.0% for AdGuard Home. These rows are maximum completed throughput under the loss gate, not low-latency recommendations.
- **SmartDNS stands out for resource use and low-concurrency efficiency.** Its selected points generally consume about one logical CPU and 6.0–22.2 MiB RSS, at the cost of earlier UDP plateaus. AdGuard Home has the lowest throughput and usually the highest CPU, RSS, and p99 in this narrow data-path matrix; that does not score its UI, client policy, or complete filtering product.
- **Repeatability supports a same-host periodic comparison except at saturated TCP q1024.** All 20 maximum stable points in the five UDP scenarios have three-run QPS coefficients of variation no greater than 4.46%. The run does not establish one overall winner or fixed multipliers for other hardware, cross-host traffic, or production upstreams.

{/* native-specialized:start */}
## Part II: OxiDNS vs mosdns equivalent-configuration suite

This section compares only OxiDNS and mosdns because both engines can load the **same YAML configuration, query file, and raw rule files** for these four paths. No `domain_set` or `ip_set` data is converted into a product-specific approximation. It was collected at `2026-07-24T16:47:08.506963+08:00` with the same versions, binary SHA-256 hashes, and server as Part I. Each point is the median of three runs; a point qualifies as stable only when median loss is at most 0.1%.

- `08-domain-set`: both load the same two geosite files directly, preserving plain suffix, `full:`, and `regexp:` rules. Timed traffic mixes ten hits with eight misses.
- `09-ip-set`: both load the same raw file containing 64 CIDRs. Four answer addresses are inside the set and four are outside it, asserting the accept and SERVFAIL outcomes.
- `42-composite-local-rewrite`: both use the same redirect → arbitrary A/AAAA → TTL rewrite → accept/reject chain. Source records have TTL 300; packet evidence asserts an output TTL of 60.
- `43-composite-provider-chain`: both use the same domain set, synthetic answers, response-IP set, and accept/reject branches, measuring the complete provider/matcher chain.

Both engines passed the A, AAAA, RCODE, and TTL checks before timing: 39 exact assertions per engine, or **78 semantic probes** in the [native-suite semantic evidence](/benchmarks/staged/native-specialized-semantic-validation.json).

### Maximum stable throughput across four paths

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/native-specialized-throughput.svg" alt="Maximum stable throughput across four equivalent OxiDNS and mosdns paths" />
  </div>
  <div className="col col--4">
    <p><strong>Higher is better, after the loss gate is satisfied</strong></p>
    <p>OxiDNS reaches 3.93× and 5.25× mosdns throughput in the domain-set and provider-chain paths. IP-set is near parity. mosdns leads the local-rewrite path by 11.7%.</p>
  </div>
</div>

### p99 at maximum stable throughput

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/native-specialized-stable-tail-latency.svg" alt="p99 latency at each engine's maximum stable throughput across four paths" />
  </div>
  <div className="col col--4">
    <p><strong>Lower is better; read it with throughput and load</strong></p>
    <p>OxiDNS has the lower p99 in all four scenarios at each engine's maximum stable-throughput point. Latency must still be read with the throughput and load on the same row rather than ranked independently of capacity.</p>
  </div>
</div>

### Native domain-set concurrency scaling

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/native-specialized-domain-scaling.svg" alt="Native domain-set throughput by concurrency" />
  </div>
  <div className="col col--4">
    <p><strong>Rising without flattening early is better</strong></p>
    <p>At loads 64 and 256 the medians are 133,302.5 versus 36,361.0 QPS and 142,929.5 versus 36,226.3 QPS—3.67× and 3.95×. mosdns largely plateaus after 64; both engines' 1024 points are excluded for exceeding 0.1% loss.</p>
  </div>
</div>

### Native domain-set p99 tail latency

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/native-specialized-domain-tail-latency.svg" alt="Native domain-set p99 tail latency" />
  </div>
  <div className="col col--4">
    <p><strong>Lower is better</strong></p>
    <p>At 256 outstanding queries, where both engines pass the loss gate, OxiDNS records 1.663 ms p99 versus 11.775 ms for mosdns. It delivers 3.95× the throughput with 85.9% lower p99.</p>
  </div>
</div>

### Native-suite CPU and memory

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/native-specialized-cpu.svg" alt="CPU at maximum stable throughput across four paths" />
  </div>
  <div className="col col--4">
    <p><strong>Read with QPS; lower is better at equal throughput</strong></p>
    <p>OxiDNS uses less CPU while delivering much more throughput in the domain-set and provider-chain paths. It uses more CPU in IP-set and local rewrite.</p>
  </div>
</div>

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/native-specialized-memory.svg" alt="RSS at maximum stable throughput across four paths" />
  </div>
  <div className="col col--4">
    <p><strong>Lower is better for the same scenario</strong></p>
    <p>OxiDNS uses less RSS in all four scenarios: 13.8–35.1 MiB versus 22.6–43.6 MiB. The per-scenario reduction is about 19%–39%, but memory should still be interpreted alongside throughput and tail latency.</p>
  </div>
</div>

### Maximum stable native-suite points

| Scenario | Engine | Outstanding | QPS | p99 | CPU | RSS | Loss |
|---|---|---:|---:|---:|---:|---:|---:|
| 08-domain-set | OxiDNS | 256 | 142,929.5 | 1.663 ms | 194.2% | 34.6 MiB | 0.0268% |
| 08-domain-set | mosdns | 64 | 36,361.0 | 4.735 ms | 329.4% | 43.4 MiB | 0.0000% |
| 09-ip-set | OxiDNS | 256 | 117,380.3 | 2.111 ms | 201.8% | 14.2 MiB | 0.0327% |
| 09-ip-set | mosdns | 256 | 112,325.5 | 2.495 ms | 159.5% | 22.6 MiB | 0.0336% |
| 42-composite-local-rewrite | OxiDNS | 256 | 98,005.1 | 2.303 ms | 219.3% | 13.8 MiB | 0.0391% |
| 42-composite-local-rewrite | mosdns | 256 | 109,458.2 | 2.431 ms | 163.7% | 22.6 MiB | 0.0348% |
| 43-composite-provider-chain | OxiDNS | 256 | 136,728.5 | 1.823 ms | 194.4% | 35.1 MiB | 0.0280% |
| 43-composite-provider-chain | mosdns | 64 | 26,063.1 | 6.271 ms | 344.2% | 43.6 MiB | 0.0000% |

### Objective assessment of the native suite

- **OxiDNS's native domain-matching advantage is clear across loads.** From outstanding-query levels 1 through 256, its three-run median is about **3.67×–4.36×** mosdns. Maximum stable throughput is **3.93×**; at the same load of 256 it is still **3.95×**, with 85.9% lower p99, so the conclusion is not an artifact of selecting different load points.
- **The complete provider/matcher chain shows the largest gap.** Maximum stable throughput is **5.25×**. mosdns reaches `0.1029%` median loss at 256, just above the shared limit. At load 64, where both have zero loss, OxiDNS is still **4.91×** faster with 1.311 versus 6.271 ms p99.
- **IP-set should be described as near parity, not a decisive lead.** At load 256, OxiDNS has 4.5% more QPS, 15.4% lower p99, and 37.0% lower RSS, but 26.5% higher CPU. Its QPS coefficient of variation at this point is 5.95%, larger than the observed 4.5% lead, so the small throughput difference is not a robust order-of-magnitude result.
- **Local rewrite provides a genuine counter-result.** mosdns delivers 11.7% more QPS with 25.3% less CPU, while OxiDNS has 5.3% lower p99 and 39.0% lower RSS. This confirms that the suite was not constructed to make OxiDNS lead every row.
- **Repeatability is usable but does not establish cross-host capacity.** Except for the OxiDNS IP-set point noted above, the other seven maximum stable points have three-run QPS coefficients of variation no greater than 2.1%. These data support a periodic same-host engineering comparison, not a fixed multiplier for every DNS workload; cold loading, rule reloads, forwarding, encrypted transports, and a separate load generator remain outside scope.
- **The four-engine matrix and two-engine suite must not be merged into one ranking.** Part I covers normalized paths all four products can express equivalently. Part II preserves native rule kinds, miss traversal, and complete policy chains where OxiDNS and mosdns share the same configuration. Both contain real measurements, but answer different questions.

Native-suite artifacts: [report](/benchmarks/staged/native-specialized-report.txt), [aggregated TSV](/benchmarks/staged/native-specialized-summary.tsv), [144 raw samples](/benchmarks/staged/native-specialized-summary.raw.json), [78 semantic probes](/benchmarks/staged/native-specialized-semantic-validation.json), and [environment snapshot](/benchmarks/staged/native-specialized-environment.json).
{/* native-specialized:end */}

## Representativeness assessment

This matrix represents stable local UDP and TCP request paths that all four products can configure with equivalent observable semantics: listener overhead, A/AAAA local answers, positive and negative warm-cache hits, and normalized domain lookup. The load sweep exposes scaling, saturation, and queueing instead of reducing the comparison to one peak-QPS number.

The deterministic cache-fill authority must receive zero requests during timed cache intervals, so public network and upstream-server capacity are excluded from those results.

It does not represent cold start or reload cost, cache-miss-heavy forwarding, DoT/DoH/DoQ, public upstream quality, multi-machine network effects, DNSSEC validation, or host-integrated side effects such as ipset/nftset. Those need dedicated matrices and, for capacity claims, a separate load-generator host.

## Interpretation limits

This is a same-host loopback comparison. It is representative of local request-path cost and concurrency scaling on the recorded machine, not public-upstream quality or production capacity on other hardware.
