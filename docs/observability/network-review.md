# Fork review and provenance

Reviewed on 2026-10-25 against `svenshi/oxidns` main at
`bd183f69348aa9c78c3b92bddbce28651d2bf66e`. This supplements PR #398; it does
not propose merging every fork feature into the observability change.

## Method and scope

The public fork inventory contained 51 repositories and 125 branch heads,
representing 79 distinct commit IDs. Upstream main, `release/v1.7.0`, and
`archive/feat/webui_standard_mode` were also inspected. The clone was
unshallowed before ancestry checks. Each head was fetched when necessary and
compared with `git merge-base --is-ancestor` and
`git log --cherry-pick --right-only --no-merges main...<head>`.

An old head that is not an ancestor is not automatically a useful new change:
rebases, patch-equivalent commits, release branches and renamed forks account
for many such heads. The inventory below records those distinctions. Shared
heads were reviewed once. Relevant individual patches and their current
callers were inspected before selecting changes.

## Adapted in this PR

| Source | Decision and implementation |
| --- | --- |
| wangmice, [728406a131ab417e8dac0760bd1b1037ba640e37](https://github.com/wangmice/oxidns/commit/728406a131ab417e8dac0760bd1b1037ba640e37), `perf(query-recorder): reduce snapshot and stream copying` | Adapt the immutable `Arc<RecordDetail>` tail/broadcast representation and the single-lock history/subscription cut. The observations endpoint clones shared handles under the tail lock, then copies observation details after releasing it. Tests check pointer identity, delivery of the next live record, and absence of replay duplicates. This is a focused adaptation to the current v1.6.1 recorder, not a cherry-pick of the full hardening branch. The source's execution-path suffix and error-allocation changes are not included. |
| ksong008, [1b73b819575795f29fbfe4a72dcf84b7fb0b8289](https://github.com/ksong008/oxidns/commit/1b73b819575795f29fbfe4a72dcf84b7fb0b8289), `serve webui html without immutable cache` | The HTML fix is already present in main. Extend that policy independently to the unversioned Next.js `.txt` route/Flight payloads. Otherwise a browser can retain route metadata from the previous deployment for a year. Hashed JS/CSS/font assets retain immutable caching. The HTTP regression test checks the metadata body and cache header as well as HTML and JS behavior. |

No fork branch is merged wholesale. These changes add no runtime dependency,
configuration option, API payload field or database migration.

## Other valuable changes reviewed

| Source | Value | Decision for PR #398 |
| --- | --- | --- |
| ksong008, [d44ecde58f72da57ad3561f89aec086d46a6d625](https://github.com/ksong008/oxidns/commit/d44ecde58f72da57ad3561f89aec086d46a6d625), `pr/query-recorder-performance` | Local reader concurrency and recorder documentation. | Already represented in main; do not duplicate it. |
| wangmice, [54f813523f3765109709b301fa712ddbd6c53fb7](https://github.com/wangmice/oxidns/commit/54f813523f3765109709b301fa712ddbd6c53fb7), `fix/query_record` | Initialization rollback; bounded cleanup; storage recovery, management and shutdown coordination. | Worth a separate recorder reliability review. Applying the entire branch would also replace lifecycle, SQLite coordination and API route registration. Those changes exceed the tracing feature and require their own failure-injection acceptance. Only the shared-snapshot/subscription subset above is adapted. |
| wangmice, [8644c11004dd](https://github.com/wangmice/oxidns/commit/8644c11004dd32eb84130efc6411ff6942599bab), `fix/err_429` | DoH 429 cooldown generations, Retry-After ownership, stale probes and publisher progress. | Relevant DNS reliability work, but independent of trace collection. Keep separate from this PR rather than changing upstream retry semantics together with the dashboard. |
| wangmice, [74a8d73459f7](https://github.com/wangmice/oxidns/commit/74a8d73459f71f713adb95a299679ef06993eaa8), `test`, and [42910acb1ba3](https://github.com/wangmice/oxidns/commit/42910acb1ba3c456e89d358d0ed43feda3a93a9e), `main` | H2 keepalive recovery, pool diagnostics, recorder hardening, dependency/release work. | Larger integration branches. The relevant recorder patch is cited individually above; no bulk merge or dependency churn. |
| xianren78, [a6db46c687b4296212daf100ea016bc5f859cea5](https://github.com/xianren78/oxidns/commit/a6db46c687b4296212daf100ea016bc5f859cea5), `fix/webui-matcher-stats` | Bounds aggregation API time ranges and preserves explicit selections. | Useful follow-up for the existing recorder statistics views. The new observations page reads a bounded memory snapshot and does not call these aggregation endpoints. Do not couple their range/API policy change to this feature. |
| xianren78, [2513066d1568](https://github.com/xianren78/oxidns/commit/2513066d1568b351458b1d0dfa826e87919d5f91), `feature/multi-endpoint`, and [faa29ce8667c](https://github.com/xianren78/oxidns/commit/faa29ce8667c791ec15eec248c01d16ca4f2f6b5), `refactor/unified-instances-page` | Multiple backends, isolated health checks, endpoint-scoped credentials and pinned plugins. | Substantial independent WebUI product work. The new page already resets polling on the existing connection epoch and uses existing authentication. Do not introduce another endpoint/store model here. |
| ksong008, [a7db371cad66e1ed30ac9d5667ce1a7fd814e381](https://github.com/ksong008/oxidns/commit/a7db371cad66e1ed30ac9d5667ce1a7fd814e381), `pr/server-inflight-limit` | Request concurrency limits. | Transport backpressure is a separate concern. Observations already cap active traces, event counts, response size by record count, and observation SSE readers. |
| kesougerui, [81c032ac773c2b0a387ccad94d54fa5c2791c348](https://github.com/kesougerui/oxidns/commit/81c032ac773c2b0a387ccad94d54fa5c2791c348), `feat/cache-prefetch` | Optional proactive refresh of frequently used cache entries near expiry. | Valuable DNS feature, but changes background query behavior, configuration and metrics. Do not add it to an observability PR. |
| iainen, [4769b7f83211](https://github.com/iainen/oxidns/commit/4769b7f8321111acde6f84715ced04adbd50ae09), `feat/docker-armv7-support` | ARMv7 container publication. | Packaging only; no change needed for the observation protocol or static WebUI. |
| qwe-qaq, [ec4d1de62115](https://github.com/qwe-qaq/oxidns/commit/ec4d1de62115e3928905a6e8c2ba44145d0d7107), `feature/openwrt`, and cicirun, [7864a53b7ddf](https://github.com/cicirun/oxidns4ksu/commit/7864a53b7ddf7132674dcba88b4d0a9db10f9eeb) | OpenWrt/Android packaging. | Independent platform integration. |
| kookxiang, [a99bda446071](https://github.com/kookxiang/oxidns/commit/a99bda446071eeefec5d1fab632378b71d524dbd), `feat/dhcp-leases` | Lease-backed local DNS answers. | Independent resolver capability; retain existing execution-step compatibility without adding a DHCP provider. |
| bigfei, [d77456b6b7b6](https://github.com/bigfei/oxidns/commit/d77456b6b7b6e0c65fbc272dd0513769926dc1ff), `codex/query-summary-ipv4` | IPv4 summary and regex redirect-template work. | Existing observation details already expose answer payloads, including IPv4; do not alter redirect behavior here. |
| svenshi, [a8d65136ddb8](https://github.com/svenshi/oxidns/commit/a8d65136ddb82112ca4cdc7d18aaae2eeb2692f0), `release/v1.7.0` | Hosts syntax, CNAME matching, diagnostics, recorder aggregations, service and packaging fixes. | Reviewed as the upcoming integration target. Keep this PR based on main; do not bring an unreleased feature train into it. |
| svenshi, [19648584b39f](https://github.com/svenshi/oxidns/commit/19648584b39f3e346899023c1eed8678d315ad90), `archive/feat/webui_standard_mode`, plus fork variants | Standard-mode policy editing and a reworked control plane. | Independent architecture/UI redesign, including older variants. Preserve the current console routes and controls instead. |

Other distinct heads contain older release history, patch-equivalent changes,
dependency automation, release workflows or renamed distributions. Their
inventory is retained below; they are not silently included or represented as
fully tested features.

## Static export cleanup

The installed output contained 107 files, 4,541,820 bytes (about 4.33 MiB):

| Files | Count | Runtime role |
| --- | ---: | --- |
| HTML | 7 | Five page entries and generated not-found entries; direct loads/refreshes use these. |
| `*.txt`, including `__next.*` | 46 | Flight payloads, route trees and prefetched layout/page segments. These are browser data, not build logs. |
| JS | 37 | Application chunks, runtime and manifests. |
| CSS | 2 | Console styles and component styles. |
| WOFF2 | 13 | Bundled font subsets. |
| PNG | 2 | Light/dark console logos. |

`!KGNvbnNvbGUp` encodes the `(console)` route group. Repeated byte contents do
not imply unused URLs: Next's static segment protocol requests the corresponding
path for each route. The installed export has no source maps, source files,
`node_modules`, test reports or `.next` compiler cache to remove.

Browser navigation against an isolated copy exercised the five routes and
confirmed requests for exported `.txt` trees/segments. It also exposed a
Windows export-path defect: the browser requested
`logs/__next.!KGNvbnNvbGUp.logs.__PAGE__.txt`, but the export contained
`logs/__next.!KGNvbnNvbGUp/logs/__PAGE__.txt`. The server's SPA fallback
returned HTML at that missing data URL; Next then fell back to the full route
payload. Checking only HTTP 200 would have missed this.

The build now runs a dependency-free post-export normalization step. It
converts nested segment payloads to the dot-encoded names required by the
[Next.js static segment protocol](https://github.com/vercel/next.js/blob/v16.2.4/packages/next/src/shared/lib/segment-cache/segment-value-encoding.ts).
This matches the exporter's intended naming in
[the corresponding export code](https://github.com/vercel/next.js/blob/v16.2.4/packages/next/src/export/index.ts).
Already-correct exports are unchanged, apart from removal of empty directories.
Target collisions fail before any payload is moved. Tests cover naming,
payload preservation, collisions and idempotence.

On the installed Windows export this renames 10 files and removes 11 empty
directories, including `_next/<build-id>`. All 107 file contents and the
`_next/static/<build-id>` manifests remain intact. All 26 distinct data URLs
observed during browser navigation subsequently returned text payloads instead
of the HTML fallback. The primary improvement is correct navigation and fewer
directories, not a claim of significant byte savings.

## Validation

The shared-record test asserts that the memory tail, initial SSE snapshot and
live receiver hold the same immutable allocation. The static HTTP test checks
unversioned route metadata cache control while keeping immutable asset caching.
Local validation after the changes:

- Rust all-feature tests: 1,307 passed, 2 intentionally ignored.
- Recorder tests: 45 passed, 2 intentionally ignored.
- GNU stable Clippy: all targets and all features, warnings denied.
- Nightly rustfmt: check passed.
- WebUI: 161 tests passed, ESLint, TypeScript and static production build passed.
- Browser: all five console routes, five observation views, direct page
  refresh, back/forward navigation, and route payload content types.

The local nightly toolchain targets MSVC and cannot link without `link.exe`.
Clippy therefore used the installed GNU stable toolchain; Windows MSVC, Linux,
macOS, nightly lint and feature combinations remain repository CI gates.

## Branch inventory

The following is a point-in-time inventory, not a claim that every historical
branch was executed. "No unique patch" means no right-side non-merge patch after
Git's patch-equivalence filtering; it can be an older or rebased history.

<details>
<summary>All 125 public fork branch heads</summary>

| Fork | Branch | Commit | Compared with main |
| --- | --- | --- | --- |
| `laincat/oxidns` | `archive/feat/webui_standard_mode` | [19648584b39f](https://github.com/laincat/oxidns/commit/19648584b39f3e346899023c1eed8678d315ad90) | 23 right-side patches |
| `laincat/oxidns` | `feat/observability-ui` | [9bc7705a3820](https://github.com/laincat/oxidns/commit/9bc7705a3820570329d23b667d160d81d1d91ed1) | 2 right-side patches |
| `laincat/oxidns` | `main` | [bd183f69348a](https://github.com/laincat/oxidns/commit/bd183f69348aa9c78c3b92bddbce28651d2bf66e) | Ancestor of main |
| `laincat/oxidns` | `release/v1.7.0` | [a8d65136ddb8](https://github.com/laincat/oxidns/commit/a8d65136ddb82112ca4cdc7d18aaae2eeb2692f0) | 21 right-side patches |
| `iainen/oxidns` | `feat/docker-armv7-support` | [4769b7f83211](https://github.com/iainen/oxidns/commit/4769b7f8321111acde6f84715ced04adbd50ae09) | 1 right-side patches |
| `iainen/oxidns` | `main` | [bd183f69348a](https://github.com/iainen/oxidns/commit/bd183f69348aa9c78c3b92bddbce28651d2bf66e) | Ancestor of main |
| `gukey/oxidns` | `main` | [bd183f69348a](https://github.com/gukey/oxidns/commit/bd183f69348aa9c78c3b92bddbce28651d2bf66e) | Ancestor of main |
| `koonuz/oxidns` | `main` | [e7137924648b](https://github.com/koonuz/oxidns/commit/e7137924648bdf1450fce5e57d6cd99fe56db5a5) | 3 right-side patches |
| `mininden19/oxidns` | `main` | [bd183f69348a](https://github.com/mininden19/oxidns/commit/bd183f69348aa9c78c3b92bddbce28651d2bf66e) | Ancestor of main |
| `kesougerui/oxidns` | `feat/cache-prefetch` | [81c032ac773c](https://github.com/kesougerui/oxidns/commit/81c032ac773c2b0a387ccad94d54fa5c2791c348) | 1 right-side patches |
| `kesougerui/oxidns` | `main` | [bd183f69348a](https://github.com/kesougerui/oxidns/commit/bd183f69348aa9c78c3b92bddbce28651d2bf66e) | Ancestor of main |
| `kirileec/oxidns` | `main` | [bd183f69348a](https://github.com/kirileec/oxidns/commit/bd183f69348aa9c78c3b92bddbce28651d2bf66e) | Ancestor of main |
| `ChitYuNG/oxidns` | `main` | [f9019c6f8237](https://github.com/ChitYuNG/oxidns/commit/f9019c6f823731f7bd090102aa31327dafdecbf4) | No unique patch |
| `biggdawg320/oxidns` | `dependabot/cargo/cargo-patch-and-minor-b32cdd65ce` | [6a715e932b39](https://github.com/biggdawg320/oxidns/commit/6a715e932b3956f4d2b68850b091ee2ee3e300fd) | 1 right-side patches |
| `biggdawg320/oxidns` | `feat/webui_standard_mode` | [41eda8236be5](https://github.com/biggdawg320/oxidns/commit/41eda8236be584d972408c3d1fecf3ff9226499e) | 23 right-side patches |
| `biggdawg320/oxidns` | `fix/mem_overflow_363` | [ad7e18f5b5bd](https://github.com/biggdawg320/oxidns/commit/ad7e18f5b5bd6f7c8e5803eeff5a4150f435cb45) | No unique patch |
| `biggdawg320/oxidns` | `jobscout/365-bug-ipv6` | [2040adf74993](https://github.com/biggdawg320/oxidns/commit/2040adf749936301c2c6f0406e0e7edd0063a44f) | No unique patch |
| `biggdawg320/oxidns` | `main` | [5e8339497679](https://github.com/biggdawg320/oxidns/commit/5e83394976790f60bd4ed0f059aa65ea1c279c21) | No unique patch |
| `starllll/oxidns` | `feat/webui_standard_mode` | [41eda8236be5](https://github.com/starllll/oxidns/commit/41eda8236be584d972408c3d1fecf3ff9226499e) | 23 right-side patches |
| `starllll/oxidns` | `fix/windows-service-restart` | [3ff14715f9fe](https://github.com/starllll/oxidns/commit/3ff14715f9feabd2b965e3822c75a3d38d0adbe3) | No unique patch |
| `starllll/oxidns` | `main` | [369e9e7c01df](https://github.com/starllll/oxidns/commit/369e9e7c01dfd5c20401cfc08d8c66114ffc6cd9) | No unique patch |
| `starllll/oxidns` | `release/v1.6.0` | [4e8fb6eb64a5](https://github.com/starllll/oxidns/commit/4e8fb6eb64a5f61833853848a5d9d5ce30a77a01) | No unique patch |
| `wangmice/oxidns` | `feat/webui_standard_mode` | [41eda8236be5](https://github.com/wangmice/oxidns/commit/41eda8236be584d972408c3d1fecf3ff9226499e) | 23 right-side patches |
| `wangmice/oxidns` | `fix/err_429` | [8644c11004dd](https://github.com/wangmice/oxidns/commit/8644c11004dd32eb84130efc6411ff6942599bab) | 218 right-side patches |
| `wangmice/oxidns` | `fix/query_record` | [54f813523f37](https://github.com/wangmice/oxidns/commit/54f813523f3765109709b301fa712ddbd6c53fb7) | 241 right-side patches |
| `wangmice/oxidns` | `fix/windows-service-restart` | [3ff14715f9fe](https://github.com/wangmice/oxidns/commit/3ff14715f9feabd2b965e3822c75a3d38d0adbe3) | No unique patch |
| `wangmice/oxidns` | `main` | [42910acb1ba3](https://github.com/wangmice/oxidns/commit/42910acb1ba3c456e89d358d0ed43feda3a93a9e) | 258 right-side patches |
| `wangmice/oxidns` | `release/v1.6.0` | [4e8fb6eb64a5](https://github.com/wangmice/oxidns/commit/4e8fb6eb64a5f61833853848a5d9d5ce30a77a01) | No unique patch |
| `wangmice/oxidns` | `test` | [74a8d73459f7](https://github.com/wangmice/oxidns/commit/74a8d73459f71f713adb95a299679ef06993eaa8) | 254 right-side patches |
| `sunbroke555/oxidns` | `main` | [8183d0fc2019](https://github.com/sunbroke555/oxidns/commit/8183d0fc20192c7d713718ffdde1c6b8b6f3ec91) | No unique patch |
| `xianren78/oxidns` | `codex/bug` | [dde6b4be23a3](https://github.com/xianren78/oxidns/commit/dde6b4be23a3f90c85461deb53e63458619b4868) | 1 right-side patches |
| `xianren78/oxidns` | `codex/bug-lycgl0` | [a994fb273cb4](https://github.com/xianren78/oxidns/commit/a994fb273cb475d14dfdf803721fb7a9c0e8f30d) | 2 right-side patches |
| `xianren78/oxidns` | `codex/implement-multi-instance-webui-for-oxidns` | [efab688afe32](https://github.com/xianren78/oxidns/commit/efab688afe3213ac894c80e07a5e4d502f7d7227) | 3 right-side patches |
| `xianren78/oxidns` | `codex/implement-multi-instance-webui-for-oxidns-9flntn` | [3588cfc53f45](https://github.com/xianren78/oxidns/commit/3588cfc53f45fb54a4692335e3294f44c1a6c53a) | 1 right-side patches |
| `xianren78/oxidns` | `codex/implement-multi-instance-webui-for-oxidns-9imj15` | [4568db4e8cd1](https://github.com/xianren78/oxidns/commit/4568db4e8cd124420acf2778b4c4c50e940ad7da) | 17 right-side patches |
| `xianren78/oxidns` | `codex/implement-multi-instance-webui-for-oxidns-amm91f` | [19486fdeec9e](https://github.com/xianren78/oxidns/commit/19486fdeec9e7faf3e8f4eba3bf60cd25a3cf7ae) | 17 right-side patches |
| `xianren78/oxidns` | `codex/implement-multi-instance-webui-for-oxidns-bgx1wg` | [627f817a1f55](https://github.com/xianren78/oxidns/commit/627f817a1f551504f1fee4adc5c47bc81ae7c713) | 1 right-side patches |
| `xianren78/oxidns` | `codex/implement-multi-instance-webui-for-oxidns-hbmvg7` | [fa6a983cd163](https://github.com/xianren78/oxidns/commit/fa6a983cd1634cede419b03dbc4d3dfec6d8d2b4) | 1 right-side patches |
| `xianren78/oxidns` | `codex/implement-multi-instance-webui-for-oxidns-j3clu8` | [d963c57157a4](https://github.com/xianren78/oxidns/commit/d963c57157a486a2969c6c62b1e97182f7a13c64) | 1 right-side patches |
| `xianren78/oxidns` | `codex/implement-multi-instance-webui-for-oxidns-ppaiei` | [4a8bd7f6c6b7](https://github.com/xianren78/oxidns/commit/4a8bd7f6c6b7f067395e02958efc23a422b0dc2e) | 1 right-side patches |
| `xianren78/oxidns` | `codex/implement-multi-instance-webui-for-oxidns-vtbgl4` | [cc712d9a325a](https://github.com/xianren78/oxidns/commit/cc712d9a325a2e032368fcbe6d965f021b14e04d) | 1 right-side patches |
| `xianren78/oxidns` | `codex/webui` | [8e3832345061](https://github.com/xianren78/oxidns/commit/8e38323450619172b3ab2110c07fcd3a05e1bf62) | 2 right-side patches |
| `xianren78/oxidns` | `dependabot/cargo/cargo-major-a925caaff2` | [6ea3f0ee0d08](https://github.com/xianren78/oxidns/commit/6ea3f0ee0d083c6c99b4ff9f99b329f760250c66) | No unique patch |
| `xianren78/oxidns` | `dependabot/cargo/cargo-patch-and-minor-f5c5d77865` | [2f1d983f6711](https://github.com/xianren78/oxidns/commit/2f1d983f671123c816a5ec9ca555326a733be161) | No unique patch |
| `xianren78/oxidns` | `feat/webui_standard_mode` | [41eda8236be5](https://github.com/xianren78/oxidns/commit/41eda8236be584d972408c3d1fecf3ff9226499e) | 23 right-side patches |
| `xianren78/oxidns` | `feature/multi-endpoint` | [2513066d1568](https://github.com/xianren78/oxidns/commit/2513066d1568b351458b1d0dfa826e87919d5f91) | 15 right-side patches |
| `xianren78/oxidns` | `fix/endpoint-dashboard-pins` | [f830a4fd3eb0](https://github.com/xianren78/oxidns/commit/f830a4fd3eb0834d37d35cae9435965051334e8d) | 17 right-side patches |
| `xianren78/oxidns` | `fix/webui-matcher-stats` | [a6db46c687b4](https://github.com/xianren78/oxidns/commit/a6db46c687b4296212daf100ea016bc5f859cea5) | 18 right-side patches |
| `xianren78/oxidns` | `fix/windows-service-restart` | [3ff14715f9fe](https://github.com/xianren78/oxidns/commit/3ff14715f9feabd2b965e3822c75a3d38d0adbe3) | No unique patch |
| `xianren78/oxidns` | `main` | [898fe00033b3](https://github.com/xianren78/oxidns/commit/898fe00033b3feb33af5b4b5c4465095675a9717) | 28 right-side patches |
| `xianren78/oxidns` | `refactor/cron-manual-run` | [32bbcb4a0b24](https://github.com/xianren78/oxidns/commit/32bbcb4a0b24cc7a2ddfdae15131bcad7993d60e) | No unique patch |
| `xianren78/oxidns` | `refactor/unified-instances-page` | [faa29ce8667c](https://github.com/xianren78/oxidns/commit/faa29ce8667c791ec15eec248c01d16ca4f2f6b5) | 18 right-side patches |
| `xianren78/oxidns` | `release/v1.6.0` | [444377c5c4da](https://github.com/xianren78/oxidns/commit/444377c5c4da74568d68e6bd9a6371dcbe11cb55) | No unique patch |
| `yjc980121/oxidns` | `main` | [8183d0fc2019](https://github.com/yjc980121/oxidns/commit/8183d0fc20192c7d713718ffdde1c6b8b6f3ec91) | No unique patch |
| `zss12345678-a/oxidns` | `main` | [bd183f69348a](https://github.com/zss12345678-a/oxidns/commit/bd183f69348aa9c78c3b92bddbce28651d2bf66e) | Ancestor of main |
| `wuzhonghai/oxidns` | `main` | [4c8557060527](https://github.com/wuzhonghai/oxidns/commit/4c8557060527917de7b45af71fe8f7d220ea47ef) | No unique patch |
| `bigfei/oxidns` | `codex/query-summary-ipv4` | [d77456b6b7b6](https://github.com/bigfei/oxidns/commit/d77456b6b7b6e0c65fbc272dd0513769926dc1ff) | 2 right-side patches |
| `bigfei/oxidns` | `codex/redirect-regex-template` | [9636037da674](https://github.com/bigfei/oxidns/commit/9636037da674c472ae0b76b1ad80bd21e543d128) | 1 right-side patches |
| `bigfei/oxidns` | `feat/webui_standard_mode` | [da4f81528cc6](https://github.com/bigfei/oxidns/commit/da4f81528cc6bfa0b0873cd38c51fcc63a4186da) | 19 right-side patches |
| `bigfei/oxidns` | `main` | [8183d0fc2019](https://github.com/bigfei/oxidns/commit/8183d0fc20192c7d713718ffdde1c6b8b6f3ec91) | No unique patch |
| `bigfei/oxidns` | `release/v1.5.2` | [7164ea9c5459](https://github.com/bigfei/oxidns/commit/7164ea9c54593e8137e3e48524f8c8c1fae76190) | No unique patch |
| `qazz-shyper/oxidns` | `main` | [8183d0fc2019](https://github.com/qazz-shyper/oxidns/commit/8183d0fc20192c7d713718ffdde1c6b8b6f3ec91) | No unique patch |
| `uvswifft/oxidns` | `dependabot/cargo/cargo-major-9fd31f0012` | [ab57cfc6067d](https://github.com/uvswifft/oxidns/commit/ab57cfc6067de5a87499d37e2103602b9ee47492) | 1 right-side patches |
| `uvswifft/oxidns` | `dependabot/cargo/cargo-patch-and-minor-0d15301f51` | [880ca206aa38](https://github.com/uvswifft/oxidns/commit/880ca206aa381476399303841dca6c034ef4feaa) | 1 right-side patches |
| `uvswifft/oxidns` | `feat/webui_standard_mode` | [556702215913](https://github.com/uvswifft/oxidns/commit/5567022159137de3d5437d2c518ca401bacfffc0) | 13 right-side patches |
| `uvswifft/oxidns` | `main` | [5e8339497679](https://github.com/uvswifft/oxidns/commit/5e83394976790f60bd4ed0f059aa65ea1c279c21) | No unique patch |
| `uvswifft/oxidns` | `pref/benchmarks` | [48b9a32403a4](https://github.com/uvswifft/oxidns/commit/48b9a32403a4b7292e530191c789885e5bcf0cde) | No unique patch |
| `uvswifft/oxidns` | `release/next_version` | [fbfdca162df8](https://github.com/uvswifft/oxidns/commit/fbfdca162df8ab9cc2d28a13c044eb410ab9844f) | No unique patch |
| `hcym/oxidns` | `dependabot/cargo/cargo-major-9fd31f0012` | [ab57cfc6067d](https://github.com/hcym/oxidns/commit/ab57cfc6067de5a87499d37e2103602b9ee47492) | 1 right-side patches |
| `hcym/oxidns` | `dependabot/cargo/cargo-patch-and-minor-0d15301f51` | [880ca206aa38](https://github.com/hcym/oxidns/commit/880ca206aa381476399303841dca6c034ef4feaa) | 1 right-side patches |
| `hcym/oxidns` | `feat/webui_standard_mode` | [556702215913](https://github.com/hcym/oxidns/commit/5567022159137de3d5437d2c518ca401bacfffc0) | 13 right-side patches |
| `hcym/oxidns` | `main` | [4c8557060527](https://github.com/hcym/oxidns/commit/4c8557060527917de7b45af71fe8f7d220ea47ef) | No unique patch |
| `hcym/oxidns` | `pref/benchmarks` | [48b9a32403a4](https://github.com/hcym/oxidns/commit/48b9a32403a4b7292e530191c789885e5bcf0cde) | No unique patch |
| `hcym/oxidns` | `release/next_version` | [fbfdca162df8](https://github.com/hcym/oxidns/commit/fbfdca162df8ab9cc2d28a13c044eb410ab9844f) | No unique patch |
| `deqiying/oxidns` | `dependabot/cargo/cargo-major-9fd31f0012` | [ab57cfc6067d](https://github.com/deqiying/oxidns/commit/ab57cfc6067de5a87499d37e2103602b9ee47492) | 1 right-side patches |
| `deqiying/oxidns` | `dependabot/cargo/cargo-patch-and-minor-0d15301f51` | [880ca206aa38](https://github.com/deqiying/oxidns/commit/880ca206aa381476399303841dca6c034ef4feaa) | 1 right-side patches |
| `deqiying/oxidns` | `feat/webui_standard_mode` | [556702215913](https://github.com/deqiying/oxidns/commit/5567022159137de3d5437d2c518ca401bacfffc0) | 13 right-side patches |
| `deqiying/oxidns` | `main` | [8183d0fc2019](https://github.com/deqiying/oxidns/commit/8183d0fc20192c7d713718ffdde1c6b8b6f3ec91) | No unique patch |
| `deqiying/oxidns` | `pref/benchmarks` | [48b9a32403a4](https://github.com/deqiying/oxidns/commit/48b9a32403a4b7292e530191c789885e5bcf0cde) | No unique patch |
| `deqiying/oxidns` | `release/next_version` | [fbfdca162df8](https://github.com/deqiying/oxidns/commit/fbfdca162df8ab9cc2d28a13c044eb410ab9844f) | No unique patch |
| `evvil/oxidns` | `main` | [4c8557060527](https://github.com/evvil/oxidns/commit/4c8557060527917de7b45af71fe8f7d220ea47ef) | No unique patch |
| `felixbennettio/oxidns-next` | `main` | [6dcddc085f6a](https://github.com/felixbennettio/oxidns-next/commit/6dcddc085f6a55e3cdc4310e833c56d39c1c553b) | 12 right-side patches |
| `lonei4ai/oxidns` | `feat/webui_standard_mode` | [9c57bf3e3e8f](https://github.com/lonei4ai/oxidns/commit/9c57bf3e3e8f24ea6334d3edecc0fab23e8057c8) | 12 right-side patches |
| `lonei4ai/oxidns` | `main` | [b844ea3df4f4](https://github.com/lonei4ai/oxidns/commit/b844ea3df4f403a173c2a56f7946fa9c9922e174) | No unique patch |
| `lonei4ai/oxidns` | `release/v1.5.0` | [690e3eeb1529](https://github.com/lonei4ai/oxidns/commit/690e3eeb15292df266a137ec7c25fe309d574097) | No unique patch |
| `kookxiang/oxidns` | `feat/dhcp-leases` | [a99bda446071](https://github.com/kookxiang/oxidns/commit/a99bda446071eeefec5d1fab632378b71d524dbd) | 1 right-side patches |
| `kookxiang/oxidns` | `feat/ros_route_plugin` | [cf98e998bf03](https://github.com/kookxiang/oxidns/commit/cf98e998bf038ab134adffb96e80386eea6d06bc) | No unique patch |
| `kookxiang/oxidns` | `feat/webui_standard_mode` | [9c57bf3e3e8f](https://github.com/kookxiang/oxidns/commit/9c57bf3e3e8f24ea6334d3edecc0fab23e8057c8) | 12 right-side patches |
| `kookxiang/oxidns` | `main` | [b844ea3df4f4](https://github.com/kookxiang/oxidns/commit/b844ea3df4f403a173c2a56f7946fa9c9922e174) | No unique patch |
| `kookxiang/oxidns` | `release/v1.5.0` | [e009aa5d6cd4](https://github.com/kookxiang/oxidns/commit/e009aa5d6cd4c2c98934128b17b4e0607aa49c71) | No unique patch |
| `wzywzy74/oxidns` | `main` | [b844ea3df4f4](https://github.com/wzywzy74/oxidns/commit/b844ea3df4f403a173c2a56f7946fa9c9922e174) | No unique patch |
| `brg3466/oxidns` | `main` | [b844ea3df4f4](https://github.com/brg3466/oxidns/commit/b844ea3df4f403a173c2a56f7946fa9c9922e174) | No unique patch |
| `istoliving/oxidns` | `main` | [b844ea3df4f4](https://github.com/istoliving/oxidns/commit/b844ea3df4f403a173c2a56f7946fa9c9922e174) | No unique patch |
| `wjs-529/oxidns` | `main` | [5e8339497679](https://github.com/wjs-529/oxidns/commit/5e83394976790f60bd4ed0f059aa65ea1c279c21) | No unique patch |
| `etplayer210/oxidns` | `main` | [28f5513b4b18](https://github.com/etplayer210/oxidns/commit/28f5513b4b18bfb8ddb945d5f42bb6fd34717faf) | No unique patch |
| `ExiaHan/oxidns` | `dependabot/cargo/cargo-patch-and-minor-da738afcc0` | [4d55de649524](https://github.com/ExiaHan/oxidns/commit/4d55de6495246e703ba528656b725577d4350f8d) | No unique patch |
| `ExiaHan/oxidns` | `feat/webui_standard_mode` | [9c57bf3e3e8f](https://github.com/ExiaHan/oxidns/commit/9c57bf3e3e8f24ea6334d3edecc0fab23e8057c8) | 12 right-side patches |
| `ExiaHan/oxidns` | `main` | [28f5513b4b18](https://github.com/ExiaHan/oxidns/commit/28f5513b4b18bfb8ddb945d5f42bb6fd34717faf) | No unique patch |
| `ExiaHan/oxidns` | `release/v1.4.1` | [57acef01b6d2](https://github.com/ExiaHan/oxidns/commit/57acef01b6d21b5bc6e9d2432daab9384bfe2649) | No unique patch |
| `geekhuyang/oxidns` | `feat/webui_standard_mode` | [9c57bf3e3e8f](https://github.com/geekhuyang/oxidns/commit/9c57bf3e3e8f24ea6334d3edecc0fab23e8057c8) | 12 right-side patches |
| `geekhuyang/oxidns` | `main` | [1b1deb003311](https://github.com/geekhuyang/oxidns/commit/1b1deb00331102c0afe3c5715bade38296a0eaee) | No unique patch |
| `geekhuyang/oxidns` | `release/v1.4.0` | [99649d9850da](https://github.com/geekhuyang/oxidns/commit/99649d9850daf8ee5eb7891af77750f030c1c350) | No unique patch |
| `paperbluster/oxidns` | `main` | [1b1deb003311](https://github.com/paperbluster/oxidns/commit/1b1deb00331102c0afe3c5715bade38296a0eaee) | No unique patch |
| `isgasho/oxidns` | `main` | [a44b96507987](https://github.com/isgasho/oxidns/commit/a44b9650798762f83ac1cb1f966406b099335c99) | Ancestor of main |
| `ohyooo/oxidns` | `main` | [8183d0fc2019](https://github.com/ohyooo/oxidns/commit/8183d0fc20192c7d713718ffdde1c6b8b6f3ec91) | No unique patch |
| `ksong008/oxidns` | `main` | [1b1deb003311](https://github.com/ksong008/oxidns/commit/1b1deb00331102c0afe3c5715bade38296a0eaee) | No unique patch |
| `ksong008/oxidns` | `pr/query-recorder-performance` | [d44ecde58f72](https://github.com/ksong008/oxidns/commit/d44ecde58f72da57ad3561f89aec086d46a6d625) | Ancestor of main |
| `ksong008/oxidns` | `pr/server-inflight-limit` | [a7db371cad66](https://github.com/ksong008/oxidns/commit/a7db371cad66e1ed30ac9d5667ce1a7fd814e381) | 2 right-side patches |
| `ksong008/oxidns` | `pr/webui-html-cache-control` | [1b73b8195757](https://github.com/ksong008/oxidns/commit/1b73b819575795f29fbfe4a72dcf84b7fb0b8289) | Ancestor of main |
| `luojiyin1987/oxidns` | `main` | [4e86e75f2c4b](https://github.com/luojiyin1987/oxidns/commit/4e86e75f2c4b7df16e9d1590060d54eb6eea6603) | Ancestor of main |
| `hu2846/oxidns` | `main` | [63810f619768](https://github.com/hu2846/oxidns/commit/63810f6197689c2d51bd9c6369b5dd2a15195761) | Ancestor of main |
| `AbyssOvO/oxidns` | `main` | [ad7f5cff3139](https://github.com/AbyssOvO/oxidns/commit/ad7f5cff313920ccb9eadc11c40b75691a262d54) | Ancestor of main |
| `yxe-king/oxidns` | `main` | [42f3f1c48b2f](https://github.com/yxe-king/oxidns/commit/42f3f1c48b2ff087c22ce8978194e1b51f41489a) | Ancestor of main |
| `hexiao5688/oxidns` | `main` | [4bafc1e6da0a](https://github.com/hexiao5688/oxidns/commit/4bafc1e6da0a262789cac9650143ef64844d1bc9) | Ancestor of main |
| `cicirun/oxidns4ksu` | `main` | [7864a53b7ddf](https://github.com/cicirun/oxidns4ksu/commit/7864a53b7ddf7132674dcba88b4d0a9db10f9eeb) | 1 right-side patches |
| `xxy5503/oxidns` | `main` | [4ffadd8b3e2c](https://github.com/xxy5503/oxidns/commit/4ffadd8b3e2c595539e13e556ee24c73501b7bfe) | Ancestor of main |
| `qwe-qaq/oxidns` | `feature/openwrt` | [ec4d1de62115](https://github.com/qwe-qaq/oxidns/commit/ec4d1de62115e3928905a6e8c2ba44145d0d7107) | 2 right-side patches |
| `qwe-qaq/oxidns` | `main` | [bd183f69348a](https://github.com/qwe-qaq/oxidns/commit/bd183f69348aa9c78c3b92bddbce28651d2bf66e) | Ancestor of main |
| `m277m277/oxidns` | `main` | [4c8557060527](https://github.com/m277m277/oxidns/commit/4c8557060527917de7b45af71fe8f7d220ea47ef) | No unique patch |
| `Glassyiris/oxidns` | `main` | [4ffadd8b3e2c](https://github.com/Glassyiris/oxidns/commit/4ffadd8b3e2c595539e13e556ee24c73501b7bfe) | Ancestor of main |
| `bibicadotnet/forgedns` | `main` | [369e9e7c01df](https://github.com/bibicadotnet/forgedns/commit/369e9e7c01dfd5c20401cfc08d8c66114ffc6cd9) | No unique patch |
| `yui0zd2-lang/forgedns` | `main` | [1d08648369cb](https://github.com/yui0zd2-lang/forgedns/commit/1d08648369cba7bd874fe4a760f4ea7fc19e5d3c) | Ancestor of main |
| `hxlls/forgedns` | `main` | [745815514e90](https://github.com/hxlls/forgedns/commit/745815514e90c5ddff457ca1e124cfc10b935ed0) | Ancestor of main |
| `wanghe1531/OxiDNS` | `main` | [bd183f69348a](https://github.com/wanghe1531/OxiDNS/commit/bd183f69348aa9c78c3b92bddbce28651d2bf66e) | Ancestor of main |
| `ShabbirHasan1/rustdns` | `main` | [2594fac12add](https://github.com/ShabbirHasan1/rustdns/commit/2594fac12add4e8afb59a33b0f45b53f67fa34b8) | No unique patch |

</details>
