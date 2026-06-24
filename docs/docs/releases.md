---
title: 版本更新
sidebar_position: 4
---

import ReleaseCard from '@site/src/components/ReleaseCard';

# 版本更新

## 2026-06

<div className="release-stack">
   <ReleaseCard version="v1.4.0" badge="Minor Release" date="2026-06-24" defaultOpen>
       **版本定位**

       - Minor Release。v1.4.0 的核心是补齐复杂网络环境下的“出口控制、上游诊断和并发裁决”能力：新增 `network.outbound` 统一出口层，新增 `oxidns probe upstream` 上游诊断命令，增强 `forward` 多上游并发结果选择，并优化缓存、DoH 入站、WebUI 升级体验和查询记录读取性能。
       - 未配置 `network.outbound.default` 的旧配置通常可以直接升级。若配置了默认 outbound profile，未显式设置 `outbound` 的 upstream 会继承该默认 profile；SOCKS5 仅作用于 TCP、DoT 和 DoH2，UDP、DoQ、DoH3 会忽略 SOCKS5 proxy。

       **主要变更**

       - `feat(network)`：新增 `network.outbound` 统一出口配置，支持集中管理 resolver nameserver、默认出口 profile 和 SOCKS5 proxy。`download`、`upgrade`、`http_request`、forward upstream、outbound resolver、Webhook 等网络路径可以复用同一套出口策略，减少多处重复配置。
       - `feat(cli)`：新增 `oxidns probe upstream <addr>` 上游诊断命令，支持上游可达性检测、域名型 upstream 解析结果展示、协议握手检查、TCP / DoT pipeline 行为判断、并发行为分类，以及 human / JSON summary。适合在启用 `network.outbound`、`bootstrap`、`pipeline` 或多协议 upstream 前验证真实链路。
       - `feat(forward)`：并发上游新增 `response_selection`，支持 `fastest`、`balanced`、`prefer_positive`、`consensus` 四种模式；`forward.concurrent` 上限提升到 `1..=32`，并按实际 upstream 数量自动裁剪。该能力用于在速度、正向答案优先、负向答案置信度和多上游一致性之间取舍。
       - `feat(cache)` / `perf(cache)`：新增 `cache.min_positive_ttl`，可跳过低有效 TTL 的正响应入缓存；同时优化 cache hit 与 TTL rewrite 热路径，降低高命中场景下的写竞争和记录复制开销，并修复 lazy refresh 低 TTL 场景下可能误删新缓存项的问题。
       - `feat(server)`：DoH 入站支持 HTTP/1.1 与 HTTP/2 自动协商，并新增入口级 `json_api`，可在 `http_server.entries[]` 上开启 RFC8484 风格以外的 `name` / `type` 查询参数支持。
       - `feat(webui)` / `fix(webui)`：WebUI 支持配置 `network.outbound` profile，并展示 outbound 相关运行时指标；升级流程新增状态展示与 Overlay，重启 / 升级检测改为识别新的 backend instance，减少快速 handoff 场景下的误判；HTML 入口改为 `no-cache`，避免升级后浏览器缓存旧页面壳。
       - `feat(query_recorder)`：query recorder 新增派生 `questions` 索引表和 `reader_concurrency`，优化 qname / qtype 过滤、top qname / qtype / latency 等 SQLite 读路径，降低大库场景下 WebUI / API 查询压力。
       - `feat(sequence)` / `docs`：`reject` 支持英文 RCODE 名称，例如 `reject NXDOMAIN`、`reject SERVFAIL`；新增显式 `reject 0 soa`；补充 DNS 编码速查表，统一说明常见 RCODE、QCLASS、QTYPE 的数字值和英文助记名。
       - `refactor` / `deps` / `ci`：整理 upstream、transport、forward 内部模块边界；升级 `hotpath`、`jiff`、`bytes`、`h2`、`webpki-roots`、`syn` 等依赖；GitHub Actions 升级到 `actions/checkout@v7`。

       **配置与升级说明**

       - 根 crate 版本号升级为 `1.4.0`；`oxidns-proto` 升级为 `0.1.3`；release tag 应使用 `v1.4.0`。
       - 从 `v1.3.0` 升级时，未配置 `network.outbound.default` 的旧配置通常可以直接升级。
       - 如果配置了 `network.outbound.default`，请检查所有未显式设置 `outbound` 的 upstream，因为它们会继承默认 profile。
       - SOCKS5 proxy 仅作用于 TCP、DoT、DoH2；UDP、DoQ、DoH3 upstream 会忽略 SOCKS5 proxy。
       - 使用自定义编译时，如需 outbound resolver 的 DoT / DoH / DoQ / DoH3 能力，请确认启用了对应 `resolver-*` feature。
       - 建议升级后对关键上游执行 `oxidns probe upstream <addr>`，尤其是使用 `network.outbound`、`bootstrap`、`pipeline`、DoH / DoQ / DoH3 或代理出口的部署。
       - `forward.response_selection`、`cache.min_positive_ttl`、`query_recorder.reader_concurrency` 均为可选配置，不设置时保持默认行为。
   </ReleaseCard>

   <ReleaseCard version="v1.3.0" badge="Minor Release" date="2026-06-16">
       **版本定位**

       - Minor Release，核心变更是将 `black_hole` 升级为覆盖全 qtype 的完整拦截器，并系统性加固 upstream 连接池、bootstrap、deadline / cancel safety 和 RouterOS 联动路径。同时完成 Rust 包结构重构：新增 `cli` 与 `infra` 层，`core` 收敛为 DNS 执行核心。配置层面保持大体兼容，但 `black_hole` 的无参默认行为与非 A/AAAA 命中语义发生变化；Rust library embedders 需要迁移公开 module path。

       **主要变更**

       - `feat(executor)`：`black_hole` 新增 `mode`（`nxdomain` / `nodata` / `null` / `custom` / `refused`），覆盖所有 qtype；无 `ips` 时默认 `nxdomain`，旧的 `ips` 写法自动按 `custom` 兼容处理。
       - `feat(upstream)`：上游连接池新增 `min_conns`，可按需保持预热连接；`max_conns` 增加明确范围校验，文档和 WebUI 配置表单同步补齐。
       - `fix(upstream)`：加固 pipeline / reuse 连接池的 deadline、取消安全、slot 回收和不可用连接裁剪逻辑，降低连接关闭、超时、替换连接失败或上游恢复期间的请求悬挂与忙等风险。
       - `fix(upstream)`：bootstrap 要求引导服务器使用字面量 IP，按 CNAME 链选择合法 A/AAAA 结果，并纳入查询 deadline；HTTP 上游请求补充 `Accept` header。
       - `feat(executor)`：`ros_address_list` 新增 `connect_timeout`、`send_timeout`、`receive_timeout`；启动阶段的 RouterOS 扫描和常驻项同步后台化，避免慢 address-list 阻塞 DNS 服务启动，并在清理前重新校验行状态。
       - `fix(matcher)`：规则文件按行解析时保留逗号，修复包含逗号的 domain / matcher 表达式被错误切分的问题。
       - `refactor`：新增 `src/cli/` 与 `src/infra/`，迁移 network、service、upgrade、build_info、error、task、cache、observability 等基础设施；`src/core/` 仅保留 `context` 与 `rule_matcher`。
       - `zoneparser`：扩展标准 RDATA 解析能力，覆盖 A/AAAA、名称类记录、MX/RT/AFSDB、TXT/SPF/AVC/RESINFO、SOA、SRV、CAA 等代表性记录，并保留 RFC3597 generic syntax 兜底。
       - `query_recorder` / 内部结构：抽出 RDATA JSON 序列化与存储辅助，降低复杂度并保持 recorder 输出路径可维护。
       - `release`：修复 GitHub Actions 上传 release 归档时的二次压缩问题，避免已打包产物被再次 archive。
       - `docs(ai)`：维护者向 AI/agent 的说明集中到 `ai/`，发布流程新增中文 GitHub Release 模板，并明确 release prep 不自动 commit / tag / push。

       **配置与升级说明**

       - 根 crate 版本号升级为 `1.3.0`；`oxidns-zoneparser` 同步升级到 `0.1.1`；`crates/macros`、`crates/proto`、`crates/ripset` 无需同步升级；release tag 应使用 `v1.3.0`。
       - `v1.2.3` 配置通常可直接升级。使用 `black_hole` 时请重点检查：旧的 `ips` 配置会自动保持 `custom` 语义；无参 `black_hole` 现在默认返回 `NXDOMAIN`；`null` / `custom` 对非 A/AAAA 返回 NODATA，而不是继续透传。
       - 如配置了 upstream `bootstrap`，现在必须使用 `IP:port`，不要写域名；新字段 `min_conns` 默认 `0`，未配置时仍保持懒加载。
       - `ros_address_list` 新增的三个 timeout 字段均为可选，默认值兼容旧配置。大型共享 address-list 仍建议拆分为 OxiDNS 专用列表，避免 RouterOS 管理面扫描成本过高。
       - Rust library embedders 需要迁移公开 module path：旧顶层 `network` / `build_info` / `upgrade` / `service` 以及 `core` 下的基础设施模块已移入 `infra`；`core::context` 和 `core::rule_matcher` 保持不变。
   </ReleaseCard>

   <ReleaseCard version="v1.2.3" badge="Patch Release" date="2026-06-11">
       **版本定位**

       - Patch Release，核心变更为修复 `/api/reload` 后 TCP / DoT 写响应任务可能空转导致的高 CPU 问题，并降低上游连接池在上游不可用或重启期间的忙等重试开销。同时补齐 WebUI 英文界面 i18n，新增升级流程中的 GitHub token 控制，并继续收敛测试与 CLI / 插件文档细节。不引入破坏性配置变更。

       **主要变更**

       - `fix(server)`：TCP / DoT 响应写入任务在连接响应通道关闭时立即退出，避免 `/api/reload` 取消连接 handler 后遗留 writer 任务持续空转；新增回归测试覆盖该路径。
       - `fix(upstream)`：pipeline / reuse 上游连接池在创建替代连接失败时加入短暂退避，避免上游故障或服务重启期间只 yield 不等待的重试循环造成 CPU 尖峰。
       - `fix(upstream)`：保持 pipeline 池仅饱和时的即时让出调度重试，避免把退避错误套用到有连接槽位即将释放的正常高并发路径。
       - `feat(webui)`：新增英文 i18n 资源与本地化 provider，控制台页面、插件定义、帮助文档和主要组件文案接入中英文资源。
       - `feat(webui)`：升级检查和应用请求支持可选 GitHub token；WebUI 新增 token 持久化控制与风险提示，并在 CLI 预览中避免暴露 token。
       - `fix(webui)`：升级入口在空闲状态下不再显示无意义的 header action。
       - `docs(cli)`：补充 `build-info` 命令文档，说明 JSON 输出、能力矩阵字段与发布排查用途。
       - `docs(plugin)`：修正插件文档中的默认值说明，保持中英文文档一致。
       - `test`：替换固定等待为确定性同步；修复 cron Windows 计时波动；在 query recorder top clients 断言前刷新 writer，降低测试偶发失败。

       **配置与升级说明**

       - 根 crate 版本号升级为 `1.2.3`；本周期 `crates/macros`、`crates/proto`、`crates/ripset`、`crates/zoneparser` 均无改动，无需子 crate 同步升级；release tag 应使用 `v1.2.3`。
       - `v1.2.2` 配置可直接升级到 `v1.2.3`，未引入新的必填配置字段或 YAML 配置迁移。
       - 长期运行且依赖 TCP / DoT 入站、频繁使用 `/api/reload`，或在上游 DNS 重启 / 不可用时观察到高 CPU 的部署，建议升级。
       - WebUI 的 GitHub token 仅用于升级检查 / 应用流程的 GitHub 请求，可选择仅本次使用或持久化保存；不配置 token 时仍沿用匿名请求行为。
   </ReleaseCard>

   <ReleaseCard version="v1.2.2" badge="Patch Release" date="2026-06-10">
       **版本定位**

       - Patch Release，核心变更为新增 HTTP 升级 API（`plugin-upgrade` feature）与 WebUI 实时更新通知，支持在 WebUI 内检测可用更新、比较版本并触发升级流程。同时修复 `${VAR}` 环境变量展开的 YAML 解析顺序问题（先解析 YAML 再展开占位符，防止 YAML 注释干扰展开），修复 WebUI 对 `${VAR}` 表单值的引号包裹处理，以及 H2/H3/DoQ 连接在远端关闭后的僵尸连接清理。不引入破坏性配置变更。

       **主要变更**

       - `feat(upgrade)`：新增 HTTP 升级 API（受 `plugin-upgrade` feature 保护），WebUI 新增更新通知横幅，可检测 GitHub 最新版本、展示当前/可用版本对比，并在 WebUI 内触发升级流程。
       - `feat(webui)`：WebUI 升级面板适配后端 plugin-upgrade 能力，整合更新检测、升级状态展示与操作入口。
       - `fix(upgrade)`：修复 apply 状态生命周期管理，并改为通过 POST body 传递所有升级参数，提升参数传递的可靠性。
       - `fix(api)`：将 upgrade 模块路由注册限定在 `plugin-upgrade` feature 开启时，避免未编译升级能力的构建暴露相关接口。
       - `fix(config)`：`${VAR}` 占位符展开改为在 YAML 解析后进行（而非之前），修复 YAML 特殊字符和注释可能干扰展开逻辑的问题；同时防止 YAML 注释文本被误作展开内容处理。
       - `fix(config)`：将 `expand_env_in_value_with_lookup` 函数提升为公开可见，供外部代码复用。
       - `fix(webui)`：修复 WebUI 在 `${VAR}` 表单字段值两端错误剥除/保留引号包裹的问题（两处相关修复）。
       - `fix(upstream)`：修复 H2（DoH）、H3（DoH3）、DoQ 连接在远端关闭后未可靠释放的僵尸连接问题，防止连接泄漏。
       - `fix(tests)`：将集成测试中的固定 sleep 等待替换为轮询等待，提升测试可靠性。
       - `fix(doc)`：修正 `${qname}` 文档注释格式。

       **配置与升级说明**

       - 根 crate 版本号升级为 `1.2.2`；本周期 `crates/macros`、`crates/proto`、`crates/ripset`、`crates/zoneparser` 均无改动，无需子 crate 同步升级；release tag 应使用 `v1.2.2`。
       - `v1.2.1` 配置可直接升级到 `v1.2.2`，未引入新的必填配置字段。
       - HTTP 升级 API 受 `plugin-upgrade` feature 保护，仅在 `standard` / `full` bundle 中可用；`minimal` 构建不受影响。
       - 部署中使用 `${VAR}` 占位符且配置文件含 YAML 注释的场景，建议升级以确保展开行为正确；旧写法无需修改，升级后行为自动改善。
       - 使用 H2/H3/DoQ 上游且长期运行的部署，建议升级以修复僵尸连接可能导致的连接泄漏。
   </ReleaseCard>

   <ReleaseCard version="v1.2.1" badge="Patch Release" date="2026-06-08">
       **版本定位**

       - Patch Release，包含 WebUI Basic Auth 登录流程与统一鉴权管理、插件画布可拖拽布局，以及多项 WebUI 交互修复（未应用插件警告、select 字段数值类型保留、查询记录流程图显示全部序列规则）。同时修复上游连接池在网络中断后的死锁问题、`${VAR}` 替换的 YAML 引号感知逻辑，并提升 `ros_address_list` 并发写入性能。不引入破坏性配置变更。

       **主要变更**

       - `ros_address_list` 性能优化：将 ROS API 写入操作并行流水化，并移除新增条目后的重查询步骤，降低大批量地址列表更新的延迟。
       - 修复 `upstream` 连接池：网络中断恢复后连接池可能进入死锁状态，本次修复避免了中断后的连接获取阻塞。
       - `feat(webui)`：新增 Basic Auth 登录流程，统一管理鉴权配置入口；登录态持久化到 `localStorage`，支持登出与会话恢复。
       - 修复 `webui`：插件未被应用时在 UI 上给出明确警告提示；抑制 404 错误噪音。
       - 修复 `config`：`${VAR}` 环境变量替换现可正确处理被 YAML 引号包裹的占位符，与裸占位符行为保持一致。
       - 修复 `webui`：select 字段在保存时保留数值类型（`number`），避免被隐式转换为字符串导致配置校验失败。
       - `feat(webui)`：插件画布支持按内容键控的拖拽布局，画布位置跟随内容标识持久化。
       - 修复 `webui`：查询记录流程图（query record flow canvas）现在显示全部序列规则，不再仅展示部分规则。
       - 依赖：Cargo patch-and-minor 组批量升级（2 个包）。
       - CI：构建环境升级到 Ubuntu 24.04；新增 release 产物收集步骤。

       **配置与升级说明**

       - 根 crate 版本号升级为 `1.2.1`；本周期 `crates/macros`、`crates/proto`、`crates/ripset`、`crates/zoneparser` 均无改动，无需子 crate 同步升级；release tag 应使用 `v1.2.1`。
       - `v1.2.0` 配置可直接升级到 `v1.2.1`，未引入新的必填配置字段。
       - 启用了管理 API 鉴权（`auth`）的部署，WebUI 登录流程会自动使用已配置的 Basic Auth 凭据；无需调整配置，升级后刷新 WebUI 页面即可看到登录界面。
       - 使用 `ros_address_list` 且有大批量地址写入需求的部署，升级后并发写入性能有所提升，无需修改配置。
       - 在 `${VAR}` 占位符周围添加了 YAML 引号（如 `value: "${MY_VAR}"`）的部署，升级后展开行为与裸占位符一致；如果此前为绕过解析问题而添加了额外引号，升级后可按需简化写法，但旧写法仍然有效。
   </ReleaseCard>

   <ReleaseCard version="v1.2.0" badge="Minor Release" date="2026-06-03">
       **版本定位**

       - Minor Release，本周期最重要的变更是引入完整的编译期特性体系（`minimal` / `standard` / `full` 三个 bundle + 细粒度 flag），将 DoQ / DoH3、DoT / DoH、`api` / `webui` / `metrics`、可选插件以及 TLS / HTTP 依赖全部改为按需启用，并把"编译进来的能力"暴露给 CLI、API 与 WebUI。同步上线两个新插件 `ip_selector`（响应 IP 选优）与 `dynamic_domain_set` + `learn_domain`（可写动态域名集与在线学习），扩展 `env` 匹配器为多条件表达式，新增 WebUI 插件卡片拖拽排序与 `dynamic_domain_set` 规则管理界面。同时修复缓存大容量场景的内存与持久化问题、DoH/DoH3 启动失败时的监听泄漏、`upgrade` WebUI 路径解析等问题，并完成多个 Cargo 依赖升级。
       - 含一个破坏性变更：`env` 匹配器移除遗留的两参数 `"KEY" "VALUE"` 解析。使用旧写法做相等匹配的配置需要迁移到 `KEY=VALUE` 形式（见下方升级说明）。

       **主要变更**

       - 编译期特性体系：新增 `minimal` / `standard`（默认推荐）/ `full` 三个 bundle，覆盖 `server-doq` / `server-doh3` / `server-dot` / `server-doh`、`upstream-doq` / `upstream-doh3` / `upstream-dot` / `upstream-doh`、`api` / `webui` / `metrics`，以及 `plugin-mikrotik` / `query-recorder` / `ipset` / `cron` / `script` / `download` / `http-request` / `reverse-lookup` / `upgrade` / `arbitrary` / `plugin-ip-selector` / `plugin-dynamic-domain`、`provider-protobuf` / `adguard-rule` 等细粒度 flag；禁用的协议/插件在配置引用时给出"未编译进来；请使用 `--features ...` 重新编译"的明确错误。最小构建的 release 二进制约 8.9 MB（full 约 21 MB，缩小约 58%）。
       - 发行物构建调整：CI 与 release 产物按 bundle 切分，新增 Linux musl 的 `minimal` / `standard` 归档（`full` 名称保持不变），`upgrade` 与安装脚本可显式选择目标 bundle。`standard` bundle 现已包含 `api`、`webui`、`query_recorder`、`upgrade` 等常用能力。
       - 运行时能力反射：CLI 与 `system/health` API 上报激活的 bundle 与支持的插件类型；WebUI 在新建、引用选择、卡片、详情视图中禁用未编译进来的插件类型。
       - 新插件 `ip_selector`（执行器）：A / AAAA 响应 IP 选优，支持 TCP / ping 带上限的探测、得分缓存、并发探测合并、DNSSEC 安全处理与失败兜底放行；拒绝兼容别名与未知字段，仅暴露 OxiDNS 原生配置项。
       - 新插件 `dynamic_domain_set`（provider）+ `learn_domain`（执行器）：基于文件的可写 provider，支持热快照、去重、API 规则管理与显式 reload；`learn_domain` 在不依赖 SQLite 与不触发整体 reload 的前提下，按过滤条件把查询/响应写入动态域名集；WebUI 新增 `dynamic_domain_set` 的规则列表、添加 / 删除 / 清空管理面板。
       - `env` 匹配器：支持在一次匹配中传入多个独立条件，每个参数都按独立表达式解析；推荐 `KEY=VALUE` 作为精确匹配语法，`KEY:VALUE` 作为别名；保留对带分隔符的 value 的兼容解析。**破坏性**：旧的两参数形式 `["KEY", "VALUE"]` 现在表示"KEY 和 VALUE 两个环境变量都存在"，不再等价于 `KEY == VALUE`。
       - WebUI 插件卡片拖拽排序：仪表盘与插件中心均支持拖拽排序。插件中心的排序会写回配置文件的 `plugins` 顺序（暂存后由"应用更改"统一保存），在当前类型 tab 内做子集排序、保留其它类型相对位置；搜索查询激活时禁用。仪表盘固定卡片顺序仅作为本地偏好持久化到 `localStorage`，不修改配置文件。
       - WebUI `ConfigField` 新增 `fullWidth` 选项并应用到 `dynamic_domain_set.path` 等长文件路径字段；修复 `@container` 查询无法样式化自身容器导致的配置表单分栏不均问题。
       - `sequence` 步骤记录改为受内部 `_sequence-step-recording` feature 控制，仅在 `query_recorder` 启用时编译进来，未启用 recorder 的构建省去对应字段与上报开销。
       - 修复 `cache`：将 `size` 视为条目上限而非启动时的 map 容量，避免大缓存配置下的预分配开销；启动与 API dump 加载后立即按上限裁剪，并增加大容量回归覆盖。
       - 修复 `server`：DoH3 / TLS 前置条件不满足或 HTTP/3 初始化失败时，先回收已启动的 HTTP/2 监听任务，避免泄漏的 DoH 监听句柄。
       - 修复 `upgrade`：从运行时配置推断 WebUI 资源路径，避免与 `working-dir` 切换组合时找不到 WebUI 资源的问题。
       - 修复 `config`：区分运行时占位符（`{...}`）与 `env` 占位符的展开规则，消除歧义。
       - 修复 `dynamic_domain_set`：序列化追加写入、分行写入新增规则、写文件前先校验规则、保持动态域名结构变更与文件状态一致；`api` 关闭时跳过对应管理接口注册。
       - 文档：新增 `PLUGIN_DEV.md` 插件开发与注册指南、`SECURITY.md` 安全策略、自定义构建中文文档与 quickstart 中的预设能力矩阵；新增 roadmap 时间线组件；移除安装文档中的 GHCR 引用；修复 TLS 配置文档格式。
       - 依赖：升级 `socket2 0.6.3 → 0.6.4`、`jiff 0.2.24 → 0.2.28`、`wincode 0.5.4 → 0.5.5`、`http 1.4.0 → 1.4.1`、`hyper 1.9.0 → 1.10.1`、`rusqlite 0.39 → 0.40`、`windows-service 0.6 → 0.8.1`。
       - 其它：`IpSelectorCacheConfig` 拒绝未知字段；修复运行时测试的序列化死锁；清理被取消的 `ip_selector` 探测；多项 CI 修复，覆盖 minimal / standard / full feature 组合与 Windows 测试；新增可复用的 custom build workflow 与最小化的 `build.config.yml` 范例。

       **配置与升级说明**

       - 根 crate 版本号升级为 `1.2.0`；本周期 `crates/macros`、`crates/proto`、`crates/ripset`、`crates/zoneparser` 均无改动，无需子 crate 同步升级；release tag 应使用 `v1.2.0`。
       - `v1.1.4` 配置在使用默认（`full`）或 `standard` bundle 升级时无需修改即可直接启动；如选择 `minimal` 或自定义裁剪 feature，配置里引用未编译进来的插件 / 协议会在启动时报错"未编译进来；请使用 `--features ...` 重新编译"，可据此添加缺失 feature 或移除对应配置项。
       - **破坏性 — `env` 匹配器**：旧的两参数写法 `env: ["KEY", "VALUE"]`（语义：`$KEY == VALUE`）需要迁移为 `env: ["KEY=VALUE"]` 或 `env: ["KEY:VALUE"]`。若刻意保留两参数语义，请确认你确实是想匹配"两个环境变量都存在"。详见 `docs/docs/migrate-from-mosdns.mdx` 中的迁移说明。
       - 选择最小化部署的用户可通过 `--no-default-features --features minimal` 或 `standard` 自行构建；release 通道同时发布 `minimal` / `standard` / `full` 三套 Linux musl 归档，`upgrade` 与安装脚本支持显式 bundle 选择。包含 WebUI、`query_recorder`、`upgrade` 的部署建议使用 `standard` 或 `full`。
       - 缓存大容量（如 `size > 200000`）部署强烈建议升级：先前的大容量场景会预分配过大的 map，并在加载 API dump 时不强制按上限裁剪；本次修复后内存占用与上限严格对齐。
       - 启用 DoH 且未启用 DoH3、或启用 DoH3 但缺少必需 TLS 配置的部署建议升级：先前在 HTTP/3 初始化失败时可能残留 HTTP/2 DoH 监听句柄；现在改为前置校验并清理已启动的任务。
       - `dynamic_domain_set` 与 `learn_domain` 为可选 `plugin-dynamic-domain` 特性、`ip_selector` 为 `plugin-ip-selector` 特性，二者均已包含在 `standard` / `full` bundle 中；最小化构建若需启用，请显式添加对应 feature。
   </ReleaseCard>
</div>

## 2026-05

<div className="release-stack">
   <ReleaseCard version="v1.1.4" badge="Patch Release" date="2026-05-30">
       **版本定位**

       - Patch Release，重点优化 provider 与规则匹配路径的内存占用与重载开销，并修复 WebUI 在移动端的配置编辑器与插件筛选可用性、查询记录图表标签显示，及 Monaco 编辑器改为本地自托管。同时新增"从 mosdns 迁移"文档。本版本不引入破坏性配置变更，查询热路径行为保持不变。

       **主要变更**

       - `client_ip` / `resp_ip` / `ptr_ip` 内联 IP 匹配器编译后改用 `finalize_compact`，不再保留一份重复的源 IP 区间副本（`ip_set` / `geoip` 此前已如此）。
       - `finalize_compact` 现在将合并后的 IPv6 区间移动进编译结构，而非克隆。
       - `geoip` 加载时通过 `add_v4_network` / `add_v6_network` 直接喂入 CIDR 字节，省去每条记录 `String` 格式化再重新解析的往返，加快加载与 reload。
       - `adguard_rule` 的 `badfilter` 解析改为一次构建 HashSet，替换原先每次比较都重新分配 cache key 的 O(n²) 扫描。
       - 修复 WebUI 配置编辑器与插件筛选在移动端无法正常使用的问题。
       - 修复 WebUI 查询记录图表 Top-N 标签被截断、无法完整显示的问题。
       - WebUI Monaco 编辑器改为本地自托管，不再从 jsdelivr CDN 加载，便于离线或受限网络环境下使用。
       - 文档：新增"从 mosdns 迁移"指南。

       **配置与升级说明**

       - 根 crate 版本号升级为 `1.1.4`；本版本无 `crates/` 子 crate 改动，无需同步升级；release tag 应使用 `v1.1.4`。
       - `v1.1.3` 配置可直接升级到 `v1.1.4`，未引入新的必填配置字段。
       - provider / 匹配器优化为内部实现改进，不改变匹配语义与查询热路径行为，无需调整配置。
       - 受限或离线网络环境下使用 WebUI 配置编辑器的部署可受益于 Monaco 本地自托管，无需访问外部 CDN。
   </ReleaseCard>

   <ReleaseCard version="v1.1.3" badge="Patch Release" date="2026-05-27">
       **版本定位**

       - Patch Release，重点修复 Linux `nftset` interval 集合写入被内核以 EINVAL 拒绝的问题、`ipset` 创建集合时 `hashsize` / `maxelem` 字节序错误，并完善 WebUI `query_recorder` 标签列表纵向溢出。同时为 `black_hole` 插件文档加入"行为将在后续版本改造"的预告。本版本不引入破坏性配置变更。

       **主要变更**

       - 修复 `nftset` interval 集合的 ADD / DEL / TEST 编码：ADD / DEL 改为 `nft` 用户态使用的两元素列表形式，解决真实内核以 EINVAL 拒绝写入的问题（issue #127）；TEST 改为仅发送起始 key，交由内核 interval 树判定包含关系。同时修正元素 timeout 的字节序，并放宽 dump 解析对孤立 `INTERVAL_END` 锚点的容错。
       - 修复 `ipset` 创建集合时 `hashsize` / `maxelem` 以本机字节序写入的问题：小端主机上 `hashsize=2048` 会被内核读成 `524288`。同时移除多余的 `IPSET_ATTR_LINENO=0` 嵌套属性，与 libipset 行为对齐。
       - 大幅扩充 `ripset` 报文格式单元测试与 ipset 集成测试覆盖。
       - 修复 WebUI `query_recorder` 详情面板长内容时标签栏纵向溢出。
       - 文档：在 `black_hole` 执行器章节加入醒目提示，说明后续版本将引入 `mode` 字段（`nxdomain` / `nodata` / `null` / `custom` / `refused`）以覆盖所有 qtype，并解释重设计的动机；现版本行为保持不变。

       **配置与升级说明**

       - 根 crate 版本号升级为 `1.1.3`；`oxidns-ripset` 同步升级到 `0.1.2`；release tag 应使用 `v1.1.3`。
       - `v1.1.2` 配置可直接升级到 `v1.1.3`，未引入新的必填配置字段。
       - 在 Linux 上使用 `nftset` 插件、且集合声明了 `flags interval` 的部署强烈建议升级，否则 ADD / DEL 会在真实内核上以 EINVAL 失败。
       - 在 Linux 上通过 OxiDNS 创建 `ipset` 集合并显式设置 `hashsize` / `maxelem` 的部署强烈建议升级；如果集合由外部 `ipset` CLI 预先创建，本次修复不影响已存在的集合。
       - `black_hole` 当前行为未变化，但建议关注后续版本的语义重设计；现阶段用于域名级拦截时推荐显式同时配置 IPv4 与 IPv6 兜底地址（如 `black_hole 0.0.0.0 :: short_circuit`），或改用 `reject 3`。
   </ReleaseCard>

   <ReleaseCard version="v1.1.2" badge="Patch Release" date="2026-05-27">
       **版本定位**

       - Patch Release，重点修复 Linux `nftset` 在 `flags interval` 集合上的写入失败、Windows 服务安装脚本问题，并完善 systemd 部署的工作目录语义、WebUI 运行日志与 `query_recorder` 排行查看体验。本版本不引入破坏性配置变更。

       **主要变更**

       - 修复 `nftset` 执行器在小端主机上读取集合 flags 时使用本机字节序，导致 `flags interval` 集合的 `is_interval` 永远为 false，所有 CIDR 写入均以 `Unsupported entry for set type` 失败的问题；改为按大端解码并新增字节序回归测试。
       - `nftset` 写入器现在按前缀独立处理：将 `IpSetError::ElementExists` 视为可跳过的 no-op，仅在结构化 warn 日志中聚合 ok / skipped / failed 计数，不再因为单次 EEXIST 整体禁用插件。
       - 修复 Debian 打包的 `systemd` 单元因 `WorkingDirectory` 预启动 CHDIR 失败的问题；现以 `-d/--working-dir` 作为运行时相对路径（包含 WebUI 资源）的唯一基准。
       - 修复 Windows 安装/卸载脚本：调整服务管理流程、二进制路径处理与卸载顺序，避免残留进程或路径异常。
       - WebUI 运行日志查看器新增整行换行开关；同时后端 `LogEntry.timestamp` 升级到毫秒精度，UI 在 `T+elapsed` 旁补充本地 `HH:MM:SS.mmm`，方便与外部时间线对齐。
       - WebUI 的 JSON 响应与 `query_recorder` SSE 流现在能容忍非 JSON 错误、心跳帧、空载与异常事件，避免控制台在偶发网络抖动时报错。
       - `query_recorder` 移除 top client / top qname / slow-query 排行接口固定的 200 行上限，WebUI 排行榜和慢查询列表新增“加载更多”按钮以支持更大的结果集。
       - WebUI 插件字段说明文档与 Rust 插件配置同步刷新。
       - 文档站点新增 Hero 组件、改进安装步骤与 Docker 运行命令展示，新增多平台快速上手指引；补齐 Debian `/etc/oxidns` 与 `/var/lib/oxidns` 目录约定、WebUI 符号链接说明与 `client_ip` 排错章节。

       **配置与升级说明**

       - 根 crate 版本号升级为 `1.1.2`；`oxidns-ripset` 同步升级到 `0.1.1`；release tag 应使用 `v1.1.2`。
       - `v1.1.1` 配置可直接升级到 `v1.1.2`，未引入新的必填配置字段。
       - 在 Linux 上使用 `nftset` 插件、且集合声明了 `flags interval` 时强烈建议升级，否则该集合在小端架构上完全无法写入。
       - 通过 deb 包升级的部署，新版本不会再设置 systemd `WorkingDirectory`；如果此前依赖该值改写相对路径，请改用 `-d/--working-dir` 显式指定。
       - 已使用 `query_recorder` 排行接口的客户端可以传入更大的 `limit`；旧的 200 行响应仍能按原方式解析，行为兼容。
   </ReleaseCard>

   <ReleaseCard version="v1.1.1" badge="Patch Release" date="2026-05-25">
       **版本定位**

       - Patch Release，重点补齐 `query_recorder` 历史记录清理能力，并修复 WebUI 插件删除流程中的交互闭环问题。本版本不引入破坏性配置变更。

       **主要变更**

       - `query_recorder` 新增 `DELETE /api/plugins/<tag>/records` 管理接口，可清空当前 recorder 的历史查询记录、执行路径 `steps` 和内存 tail；清空前会先 flush 后台写入队列，并返回 `cleared_records`。
       - WebUI 查询记录面板新增“清空历史”按钮，提供二次确认、清空中状态反馈，并在完成后刷新记录列表、选中详情和插件命中统计。
       - WebUI 插件删除弹窗体验优化：引用提示弹窗加宽，长字段可换行，并更清晰展示引用来源、目标类型和不可安全移除的原因。
       - 修复插件删除弹窗取消后误打开插件详情抽屉的问题。
       - 修复“进入编辑器修复”会提前从插件中心移除插件的问题；现在该操作只进入编辑器，由用户手动处理引用。
       - 修复配置存在错误时删除 icon 常显且无法点击的问题；现在仍可打开弹窗查看错误原因。

       **配置与升级说明**

       - 根 crate 版本号升级为 `1.1.1`；release tag 应使用 `v1.1.1`。
       - `v1.1.0` 配置可直接升级到 `v1.1.1`，未引入新的必填配置字段。
       - `query_recorder` 清空历史是可选管理能力，不影响现有查询记录采集、保留期清理或统计查询行为。
       - “清空历史”操作不可撤销，会删除当前 recorder 已持久化的查询记录和路径事件；生产环境建议确认不再需要这些审计数据后再执行。
   </ReleaseCard>

   <ReleaseCard version="v1.1.0" badge="Minor Release" date="2026-05-25">
       **版本定位**

       - Minor Release，重点增强配置安全性、升级与重启流程、`query_recorder` 统计分析和 WebUI 运维体验，同时补齐插件文档导航与路线图。本版本包含 `upgrade` 配置的破坏性变更，请升级前检查相关配置或自动化脚本。

       **破坏性变更**

       - `upgrade` 的重启配置已从枚举型 `restart: none|service` 和 CLI 参数 `--restart <none|service>` 改为布尔型 `no_restart: true` 与 `--no-restart`。
       - 默认行为也同步变化：`upgrade apply` 成功后现在会自动重启服务；需要保持旧版本“不自动重启”行为时，必须显式设置 `no_restart: true` 或传入 `--no-restart`。

       **主要变更**

       - 配置加载链路支持 YAML 环境变量占位符：`${VAR}`、`${VAR:-default}` 和 `$${...}`。占位符会在启动、`oxidns check`、管理 API 校验和保存前校验时展开，支持 `include` 路径，并在变量缺失或语法错误时报告变量名、行号和列号。
       - `upgrade` 流程重构为跨平台 apply：Windows 现在支持 `.zip` archive 解包、二进制替换和 WebUI 目录升级；zip 解包会拒绝不安全路径，避免 zip-slip。
       - `upgrade` 新增 GitHub token 支持，可用于提高 API 速率限制或访问私有仓库；CLI 使用 `--github-token`，插件配置使用 `github_token`。
       - `upgrade` 成功后默认重启服务：CLI 通过系统服务管理器重启已安装服务，插件内升级通过应用控制通道触发优雅重启并加载新二进制。需要跳过重启时，CLI 使用 `--no-restart`，插件配置使用 `no_restart: true`。
       - 应用控制面新增 `POST /restart`，进程在 Unix 上通过 `exec` 原地重启，在 Windows 服务场景下配合 SCM 重启，并在二进制替换前捕获原始可执行文件路径，避免 Linux 上 `/proc/self/exe (deleted)` 导致重启失败。
       - `query_recorder` 新增聚合统计 API 和 WebUI 图表：Top clients、Top qnames、qtype / rcode 分布、延迟直方图、慢查询排行和按分钟/小时聚合的查询趋势；SQLite 读写参数也针对统计查询做了优化。
       - WebUI 配置应用生命周期更清晰：顶层 `runtime` / `api` / `log` 等变更会提示重启而不是热重载；配置回滚会根据变更类型自动选择热重载或重启，并在重启过程中展示连接恢复状态。
       - WebUI 插件管理增强：删除插件前会检查依赖引用，可选择替换引用、移除可安全删除的引用，或进入编辑器手动修复；插件重命名会同步更新引用并在有影响时要求确认。
       - 文档更新插件总览和侧边栏导航，新增路线图页面，并补充 `redirect` 规则形式、`qname` / `cname` 域名规则说明、README 路线图与免责声明。

       **配置与升级说明**

       - 根 crate 版本号升级为 `1.1.0`；release tag 应使用 `v1.1.0`。
       - `v1.0.2` 的 DNS 解析配置通常可直接升级到 `v1.1.0`；环境变量占位符是新增能力，不使用占位符的配置行为保持不变。
       - Breaking Change：旧的 `restart: none|service` 和 CLI `--restart <none|service>` 已不再接受；请改用 `no_restart: true` / `--no-restart`。如果希望保持旧版本“升级后不自动重启”的行为，需要显式设置 `no_restart: true` 或传入 `--no-restart`。
       - `${VAR}` 占位符缺失会阻止配置解析；需要保留字面量 `${...}` 时写作 `$${...}`，环境变量值包含 YAML 特殊字符时建议给占位符加引号。
       - 新增 `github_token` / `--github-token` 为可选字段，不影响现有公开仓库升级配置。
       - 已启用 `query_recorder` 的部署无需修改配置即可使用新增统计 API 和 WebUI 图表；统计查询会读取 SQLite 历史数据，数据库较大时建议关注磁盘与查询延迟。
   </ReleaseCard>

   <ReleaseCard version="v1.0.2" badge="Patch Release" date="2026-05-21">
       **版本定位**

       - Patch Release，修复域名型 upstream 在启动和配置校验阶段依赖本机 DNS 的问题，并明确 `bootstrap` 与 `dial_addr` 的解析优先级。

       **主要变更**

       - 修复 `forward` 插件地址校验复用完整 `ConnectionInfo` 构造逻辑的问题。域名型 upstream 现在只做地址格式校验，不会在启动校验阶段触发系统 DNS 解析。
       - 调整 upstream 连接信息构造：仅字面 IP 和显式 `dial_addr` 会在启动阶段写入连接目标 IP；域名保留为 `server_name`，后续由 `bootstrap` 或首次建连时的系统解析处理。
       - 明确 `dial_addr` 与 `bootstrap` 同时配置时的互斥行为：`dial_addr` 优先生效，`bootstrap` 会被忽略，并在初始化时输出 warning。
       - 更新 `forward` 插件参考文档和 WebUI 插件字段说明，补充域名解析时机、`bootstrap` / `dial_addr` 二选一建议以及运行期优先级。
       - 补充回归测试，覆盖域名 upstream 不预解析、`dial_addr` 保留 SNI 主机名，以及 `dial_addr` 覆盖 `bootstrap` 的行为。

       **配置与升级说明**

       - 根 crate 版本号升级为 `1.0.2`；release tag 应使用 `v1.0.2`。
       - `v1.0.1` 配置可直接升级到 `v1.0.2`，未引入新的必填配置字段。
       - 未配置 `bootstrap` 或 `dial_addr` 的域名型 upstream 不再阻塞启动；首次建连时仍会使用系统解析。
       - 如需完全避免运行期对本机 DNS 的依赖，域名型 upstream 建议在 `bootstrap` 和 `dial_addr` 中二选一配置。
       - 同时配置 `bootstrap` 和 `dial_addr` 的现有配置仍可启动，但只有 `dial_addr` 生效。
   </ReleaseCard>

   <ReleaseCard version="v1.0.1" badge="Patch Release" date="2026-05-20">
       **版本定位**

       - Patch Release，修复 `v1.0.0` 中的 DNS 响应合规性问题、客户端 IP 规范化缺陷和 WebUI 使用问题，同时新增服务管理能力、安装器脚本和查询审计交互改进。

       **主要变更**

       - 修复 `redirect` 插件合成 DNS 响应中 CNAME 未置于 answer section 首位的问题，确保与 RFC 规范对齐。
       - 修复双栈 socket 接收 IPv4-mapped IPv6 地址（`::ffff:x.x.x.x`）时，`DnsContext` 未将其规范化为真实 IPv4 地址，导致 `client_ip` 匹配器等依赖 IP 的逻辑错判。
       - 修复 WebUI 刷新页面出现 404 的问题，并在首次加载时自动连接 `/api` 后缀以适配全量后端托管场景。
       - 新增 `service restart` 命令，支持以系统服务方式运行时通过 CLI 重启 OxiDNS。
       - 新增 Linux / macOS / Windows 托管服务安装器脚本（`install.sh` / `install.ps1`），实现一条命令安装、注册并启动系统服务。
       - `query_recorder` 面板新增按 matcher 行点击过滤查询记录的能力，补充延迟着色视觉提示和 record-count 列名信息浮层。
       - WebUI 插件详情面板、缓存管理对话框和配置字段编辑器整体打磨：替换原生 `confirm` 为 shadcn AlertDialog、采用响应式双列布局、居中约束 max-w-6xl 内容区域。

       **配置与升级说明**

       - 根 crate 版本号升级为 `1.0.1`；release tag 应使用 `v1.0.1`。
       - `v1.0.0` 配置可直接升级到 `v1.0.1`，未引入新的必填配置字段。
       - 使用双栈 socket 且依赖 `client_ip` 匹配器、ECS 或 IP 相关策略的部署，升级后客户端 IP 将正确规范化为 IPv4。
       - 安装脚本默认将应用注册为系统服务；仅需便携安装时设置 `OXIDNS_INSTALL_SERVICE=0`。
   </ReleaseCard>

   <ReleaseCard version="v1.0.0" badge="Major Release" date="2026-05-19">
      **版本定位**

      - Major Release，标志 OxiDNS 从实验性插件化 DNS 引擎进入 1.0 稳定阶段。`v1.0.0` 正式内置 WebUI 管理控制台，并包含 `v0.5.2` 以来的管理 API、插件运行时、观测能力、发布打包和性能稳定性改进。

      **重要升级提醒**

      - `v1.0.0` 完成项目名迁移，项目正式更名为 OxiDNS，GitHub 仓库、release asset、二进制文件名、包元数据、服务文件、README、文档站、logo 与启动 banner 均已切换到 `oxidns`。
      - 由于旧版本的自动更新逻辑仍指向更名前的项目与旧 release 资产，无法直接更新到 `v1.0.0`。从旧版本升级到 `v1.0.0` 时，必须手动下载对应平台的 OxiDNS release 包，替换二进制文件，并同步部署包内 WebUI 静态资源。
      - 完成这次手动迁移后，后续版本应使用新的 `svenshi/oxidns` 仓库和 `oxidns-*` release 资产进行升级。

      **WebUI 能力**

      - OxiDNS WebUI 将运行状态、配置、插件、指标、日志、查询审计和缓存管理集中到一个控制台中，日常运维不再依赖分散的命令行、日志文件和手写 API 调用。
      - 配置管理更适合生产环境：YAML 编辑、在线校验、配置历史、diff、应用与回滚集中在同一流程内，复杂策略调整可以先确认再生效，降低误改配置后的恢复成本。
      - 插件编排更直观：插件拓扑、插件详情、字段化配置和 sequence composer 帮助理解 DNS 请求处理链路，减少手写 YAML 时的引用错误、依赖遗漏和排查成本。
      - 故障定位更直接：指标、在线日志、查询记录、执行流和缓存详情可以串联查看，便于从异常域名追踪到命中规则、上游行为、缓存状态和最终响应。
      - 部署与访问更简单：WebUI 随 release archive、Docker 镜像、Debian 包和 `upgrade` 流程分发，也可由 OxiDNS 管理 API 直接托管静态资源，单个 OxiDNS 进程即可同时服务 API 与控制台。

      **主要变更**

      - 管理 API 重构为带前缀的统一入口，新增认证/CORS、runtime state、日志流、指标、配置保存/应用/回滚、插件 API 汇聚等能力。
      - 插件体系从全局可变 registry 重构为不可变 catalog + runtime manager，插件工厂创建上下文更简洁，reload 路径更稳健，并将 registry 拆分为 catalog、context、init_plan、runtime 等模块。
      - 新增共享插件指标层，覆盖 server、forward upstream、cache、query recorder、side-effect executor 等路径，并统一暴露给管理 API。
      - `query_recorder` 具备 matcher 命中采样统计、过滤查询、执行流可视化和记录详情展示能力；相关 model 和 store 结构完成清理。
      - cache 新增管理 API，可读取缓存 DNS 响应详情、TTL、命中信息、记录内容和缓存快照。
      - `upgrade`、release、Docker、Debian packaging、systemd service 和 CI workflow 更新为 OxiDNS 1.0 发布链路。
      - 项目品牌从 ForgeDNS 完成迁移到 OxiDNS，更新 GitHub 模板和所有面向用户的项目标识。
      - 性能与稳定性方面，TCP upstream 禁用 Nagle、减少 split lock；dual-selector 探测逻辑从 forward 中解耦；支持 dual-stack port-only listener；全局 runtime manager reload 更稳健。
      - 文档系统更新 README、quickstart、configuration、API、plugin reference、scenarios、benchmarks 和 MikroTik policy routing。

      **配置与升级说明**

      - 根 crate 版本号升级为 `1.0.0`；release tag 应使用 `v1.0.0`。
      - 现有 `v0.5.2` DNS 解析配置通常可直接升级；`v1.0.0` 主要引入完整 WebUI、管理 API、指标和打包能力。
      - 从旧项目名版本升级时，不能依赖旧的自动更新流程跨过项目更名边界；请手动下载 `v1.0.0` release 包并完成迁移。
      - 管理 API 入口已收敛为带前缀的路由；如果外层反向代理、ACL 或脚本直接调用旧 API 路径，需要按新版 API 文档确认路径。
      - 使用自动升级、Docker 或 Debian 包部署时，建议同时确认控制台静态资源目录和服务文件已随新包安装。
      - 依赖插件 reload、配置在线编辑或运行时 API 的部署，建议先在测试环境验证权限、CORS、认证和回滚流程。
   </ReleaseCard>

   <ReleaseCard version="v0.5.2" badge="Patch Release" date="2026-05-04">
      **版本定位**

      - Patch Release，重点修复 DoH / DoH3 上游长连接复用和 upstream duration 配置解析问题。

      **主要变更**

      - 修复 DoH（HTTP/2）和 DoH3（HTTP/3）上游连接池可能复用已关闭连接的问题。远端关闭空闲连接后，连接池会及时淘汰失效连接并重建可用连接，避免后续查询持续出现 `H2 send_request error` 或 `H3 send_request error`（Closed #78）。
      - 修复 upstream `timeout` 字段无法从配置文件正确解析的问题。`timeout: 3` 和 `timeout: "3s"` 均可正常反序列化并用于 forward 插件初始化（Closed #79）。
      - 补充统一的 duration 配置解析逻辑，支持 `ms`、`s`、`m`、`h`、`d` 等单位，未带单位的数字默认按秒处理。

      **配置与升级说明**

      - 本次发布不引入新的必填配置字段，`v0.5.1` 配置可直接升级。
      - `timeout`、`idle_timeout` 等 duration 配置项支持 `3`、`"3"`、`"3s"`、`"500ms"` 等写法。
      - 未带单位的 duration 数字会按秒解析；毫秒级配置应显式使用 `ms` 后缀。
      - 对于配置了 upstream `timeout`，或使用 DoH / DoH3 上游并遇到长时间运行后持续请求失败的部署，建议升级到 `v0.5.2`。
  </ReleaseCard>
</div>

## 2026-04

<div className="release-stack">
  <ReleaseCard version="v0.5.1" badge="Patch Release" date="2026-04-28">
      **版本定位**

      - Patch Release，重点修复 `any_match` quick setup 依赖分析和 `query_recorder` 分页、清理边界。

      **主要变更**

      - 修复 `any_match` 在依赖分析阶段会丢失 quick setup 表达式的问题。`qname $provider`、`qtype 1` 等 quick setup matcher 会按原表达式解析并展开依赖。
      - 修复 `query_recorder` 的保留期清理与分页游标边界。清理截止时间改为基于真实时间戳计算，分页列表会多取一条记录判断是否还有下一页。
      - 调整 `query_recorder` 记录时间字段的存取类型，避免时间戳在写入、读取和清理路径中发生不必要的无符号转换。
      - 同步修正 `upgrade` CLI 默认缓存和备份目录为 `./upgrade-cache` 与 `./upgrade-backups`，并修复对应默认值测试。

      **配置与升级说明**

      - 本次发布不引入新的配置字段，`v0.5.0` 配置可直接升级。
      - 已启用 `query_recorder` 或在 `any_match` 中使用 quick setup 表达式的部署，建议升级到 `v0.5.1`。
      - `query_recorder` 仍处于 **Experimental** 阶段，其 API 与配置字段后续仍可能调整。
  </ReleaseCard>

  <ReleaseCard version="v0.5.0" badge="Minor Release" date="2026-04-27">
      **版本定位**

      - Minor Release，新增查询审计能力、组合 matcher 能力，并增强 HTTP/3 发现体验。

      **主要变更**

      - 新增 `query_recorder` executor，支持将查询记录落盘、按保留策略清理，并通过插件 API 查询统计、分页读取和单条记录详情。
      - 新增 `any_match` matcher，支持在一个 matcher 中聚合多条 matcher 表达式，只要任意一条命中即返回 true，并支持 `!$tag` 形式的否定表达式。
      - HTTP server 在启用 HTTP/3 时，会在 HTTP/2 响应中自动宣告 `Alt-Svc: h3=":<listen-port>"; ma=86400`，帮助客户端发现并升级到 H3。
      - 修复 `sequence` 中否定 matcher（如 `!$has_resp`）未正确纳入依赖跟踪的问题，避免 quick setup / 依赖分析阶段遗漏引用（Closed #75）。
      - 时间相关逻辑统一到 `jiff + AppClock`，使 cron 触发、日志时间和系统时间获取路径更一致。

      **配置与升级说明**

      - 本次发布不引入必须变更的全局配置字段，现有 `v0.4.x` 配置可直接升级。
      - `query_recorder` 当前为 **Experimental** 能力，后续小版本中其 API 与配置字段可能调整。
      - 启用查询审计时，可在 `sequence` 中按需插入 `query_recorder`，并结合 retention 参数控制磁盘占用。
      - 启用 DoH 客户端自动发现 HTTP/3 时，应确认 HTTP server 已设置 `enable_http3: true` 且证书配置完整。
  </ReleaseCard>

  <ReleaseCard version="v0.4.2" badge="Patch Release" date="2026-04-24">
      **版本定位**

      - Patch Release，修复上游竞争场景的连接释放问题，并新增自动升级能力。

      **主要变更**

      - 修复在配置多个并发 upstream、启用 fallback 等存在上游竞争的场景下，部分连接未被正确释放的问题。
      - 新增 `upgrade` CLI 工具及插件，支持自动更新并替换二进制文件。
      - 应用以 Linux Service 方式运行时，`upgrade` 支持更新后自动重启应用。

      **配置与升级说明**

      - 本次发布不引入新的必填配置字段。
      - 依赖多 upstream 并发竞争、fallback 或自动升级流程的部署可升级到 `v0.4.2`。
  </ReleaseCard>

  <ReleaseCard version="v0.4.1" badge="Patch Release" date="2026-04-23">
      **版本定位**

      - Patch Release，修复 upstream `request_map` 内存泄漏，并提升 DoH HTTP 响应兼容性。

      **主要变更**

      - 修复 upstream `request_map` 在连接关闭、请求超时和异常回收场景下的内存泄漏问题，避免 pending query waiter 与 sender 残留。
      - 重写 `request_map` 为固定容量的稀疏表实现，不再为每条连接预留完整 `u16` DNS ID 空间。
      - 修复 DoH 响应头生成逻辑：`application/dns-message` 响应会写入正确的 `Content-Length`，并按实际 DNS TTL 生成 `Cache-Control: max-age=...`。
      - `NoError`、`NXDOMAIN`、`NODATA` 等常见 DoH 响应会分别从 answer TTL 或 SOA negative TTL 推导 HTTP 缓存时间；拒绝类响应不再强行附带误导性缓存头。

      **配置与升级说明**

      - 本次发布不引入新的配置字段，`v0.4.0` 配置可直接升级到 `v0.4.1`。
      - 由于修复的是 upstream `request_map` 的内存泄漏问题，建议长期运行、长连接较多或上游并发较高的部署升级到 `v0.4.1`。
      - 通过 `dig +https://...`、浏览器、反向代理或网关缓存访问 DoH 的场景，升级后可获得更稳定的 HTTP 响应兼容性。
  </ReleaseCard>

  <ReleaseCard version="v0.4.0" badge="Minor Release" date="2026-04-19">
      **版本定位**

      - Minor Release，新增 provider 级热刷新能力，并重构 provider 组合与初始化模型。

      **主要变更**

      - 新增 `reload_provider` executor，以及 provider 级管理接口 `POST /plugins/<provider_tag>/reload`。下载或覆盖规则文件后，可以只刷新受影响的 provider，而不必触发应用级全量 `reload`。
      - 重构 provider 组合模型：`domain_set` / `ip_set` 只编译自身本地规则，运行时继续查询 `sets` 中引用的 provider。
      - runtime 初始化会跳过没有 live dependents 的 provider，避免未被消费的规则集在启动阶段做无意义的文件读取、dat 解析和内存占用。
      - quick setup 依赖分析扩展到 `sequence` / `cron` 等运行时引用场景，使插件依赖图与初始化顺序更准确。
      - 文档新增 targeted provider reload 的 API 与 `reload_provider` executor 说明，并补充下载后刷新 provider 的串联示例。

      **配置与升级说明**

      - 现有“`download` 覆盖文件后再全量 `reload`”的流程，通常可以改为“`download -> reload_provider`”，降低对其它插件的重建影响。
      - `reload_provider` 只适用于刷新 provider 的既有配置和外部数据文件；变更涉及 `config.yaml`、provider tag、`sets` 拓扑或插件列表时，仍需要使用全量 `reload`。
      - 未被任何 live 路径引用的 provider 将不会进入 runtime registry；依赖其运行时 API 或行为时，应确保它被 `server`、`executor`、`matcher` 直接或间接引用。
  </ReleaseCard>

  <ReleaseCard version="v0.3.2" badge="Patch Release" date="2026-04-16">
      **版本定位**

      - Patch Release，降低正常连接生命周期产生的误报日志，并改善调试输出。

      **主要变更**

      - 调整 UDP、TCP、DoT、DoQ 上游连接池的初始化策略，不再在启动时预创建空闲连接，减少部分上游主动关闭空闲连接时产生的误报 EOF / reset 日志。
      - TCP 上游连接复用流程将预期内的 EOF、连接回收和失效连接淘汰视为 `debug` 级事件，避免正常连接生命周期被误记为告警。
      - DoH 服务端将浏览器或代理主动中断引发的 TLS、HTTP/2、HTTP/3 握手失败，以及客户端提前关闭响应流导致的发送失败，下调为 `debug` 日志。
      - Debug 日志中的 DNS 请求与响应信息现在直接输出 `questions`、消息 ID、EDNS 和 answers 内容；`Record` 新增更易读的 `Debug` / `Display` 输出格式。

      **配置与升级说明**

      - 本次发布不引入新的配置字段，现有 `0.3.x` 配置可直接升级。
      - 监控依赖 warning 日志计数时，升级到 `v0.3.2` 后，正常的上游断连和 DoH 客户端中断将不再放大告警噪音。
  </ReleaseCard>

  <ReleaseCard version="v0.3.1" badge="Patch Release" date="2026-04-14">
      **版本定位**

      - Patch Release，修正 `sequence` 内建控制流语义，并补齐发布元数据。

      **主要变更**

      - 修正 `sequence` 的内建控制流语义：`accept` / `reject` 稳定终止当前链路，`return` 显式返回调用方，`jump` 与 `goto` 在嵌套 `sequence` 中的恢复行为更一致。
      - 移除依赖内部 flow state 的控制方式，改为由 `ExecStep` 显式传播控制流结果，减少 `sequence`、`with_next` executor 和嵌套调用混用时的语义歧义。
      - 补强 `sequence` 的单元测试与集成测试，覆盖 `accept`、`return`、`reject`、`jump`、`goto` 以及 `adguard_rule` / `question` 组合分支。
      - 为 `oxidns-proto`、`oxidns-zoneparser`、`oxidns-ripset` 补齐 crates.io 发布所需的包元数据、README、仓库信息和依赖版本声明。
      - 更新 `configuration`、`executor`、`matcher` 文档，对 `sequence` 内建控制流、`mark` 语法，以及 `qtype` / `qclass` 数值写法给出更明确说明。

      **配置与升级说明**

      - 配置依赖嵌套 `sequence`、`jump` / `goto` / `return` 组合时，建议升级到 `v0.3.1` 以获得更稳定且可预测的控制流行为。
      - 本次发布不引入新的配置字段，主要是控制流修正、测试补强和发布元数据整理。
  </ReleaseCard>

  <ReleaseCard version="v0.3.0" badge="Minor Release" date="2026-04-14">
      **版本定位**

      - Minor Release，新增 HTTP 回调、配置检查、dat 导出、zone 解析和 Linux netlink 集成能力。

      **主要变更**

      - 新增 `http_request` executor，支持在 `before/after` 两个阶段向外部 `http/https` 服务发起同步或异步回调，并支持模板变量、`json/form/body`、SOCKS5、重定向和错误策略。
      - CLI 新增 `check` 与 `export-dat` 命令；`check --graph` 可静态校验配置并输出插件依赖图，`export-dat` 可把 `geosite.dat` / `geoip.dat` 按 selector 导出为 OxiDNS 或原始文本规则。
      - `hosts` 语义向 mosdns 对齐；`arbitrary` 引入更完整的 zone parser，支持 `$ORIGIN`、`$TTL`、`$INCLUDE`、`$GENERATE`、RFC3597 等更丰富记录语法。
      - Linux `ipset` / `nftset` executor 改为内置 Rust netlink 后端，不再依赖运行时 `ipset` / `nft` 命令。
      - workspace 新增 `oxidns-proto`、`zoneparser`、`ripset` 三个内部 crate；网络热路径引入可复用 wire buffer 池，并优化 UDP/TCP/上游 socket 参数。
      - docs 新增 CLI 页面，并更新 `executor`、`provider`、`quickstart`、`benchmarks`、`releases` 等章节。

      **配置与升级说明**

      - `hosts` 中无前缀规则现在等价于 `full:`；正向本地答案 TTL 固定为 `10`；域名命中但地址家族不匹配时会返回 `NoError + 空 Answer + fake SOA`，默认不再透传后续 executor。
      - `arbitrary` 不再提供旧 quick setup 语法，升级时建议改为显式 `rules` / `files` 配置。
      - quickstart 新增 Docker Compose 示例，补充 Docker 镜像仓库、Windows release 资产与服务部署说明。
  </ReleaseCard>

  <ReleaseCard version="v0.2.1" badge="Patch Release" date="2026-04-03">
      **版本定位**

      - Patch Release，修复 DoH over HTTP/2 上游 GET 请求问题，并补充 quickstart 文档。

      **主要变更**

      - 修复 DoH over HTTP/2 上游 GET 请求未正确结束 stream，导致部分上游在 5 秒后超时的问题。
      - 完善 `Question` 的 `Display` 输出，统一日志和调试信息中的查询展示格式。
      - 放宽 cache TTL 单测中的时间边界假设，避免 CI 在跨秒时出现偶发失败。
      - quickstart 文档移除 Docker `linux/arm/v7` 支持说明，并新增 `docker compose` 部署示例。

      **配置与升级说明**

      - 本次发布不引入新的配置字段。
      - 使用 DoH over HTTP/2 上游 GET 请求的部署建议升级到 `v0.2.1`。
  </ReleaseCard>

  <ReleaseCard version="v0.2.0" badge="Feature Release" date="2026-04-02">
      **版本定位**

      - Feature Release，新增订阅下载、定时任务、脚本执行和 geodata provider 能力。

      **主要变更**

      - 新增 `download` executor，支持将远程 `http/https` 文件下载到本地目录，并支持 SOCKS5 代理、HTTP 重定向跟随、启动时自动补齐缺失文件。
      - 新增 `cron` executor，可按固定间隔或标准 5 字段 cron 表达式执行后台任务。
      - 新增 `reload` executor，可触发一次完整的应用级 reload。
      - 新增 `script` executor，可执行外部命令并注入稳定上下文字段。
      - 新增 `geoip`、`geosite`、`adguard_rule` provider，以及 `question` matcher；`qname` 域名匹配新增对 `adguard_rule` 规则集的支持。
      - cache 新增 `stale lazy refresh` 行为，rule matcher 完成结构拆分与热路径优化，并新增可配置日志文件轮转能力。
      - 系统更新 `executor`、`matcher`、`provider`、`server`、`quickstart`、`scenarios` 等文档，并新增 docs-site CI。

      **配置与升级说明**

      - `startup_if_missing` 默认启用，更适合首次部署和规则文件自举场景。
      - `ros_address_list` 支持 `fixed_ttl=0`，表示无超时。
      - `hosts`、`black_hole`、`cache` 的 quick setup 新增 `short_circuit` 支持。
      - 移除 `hosts` quick setup，收敛早期不够稳定的快速配置入口。
      - 从 `serde_yml` 迁移到 `serde_yaml_ng`，并同步更新部分依赖和 CI 工具链。
  </ReleaseCard>
</div>

## 2026-03

<div className="release-stack">
  <ReleaseCard version="v0.1.1" badge="Compatibility Update" date="2026-03-29">
      **版本定位**

      - Compatibility Update，统一 MikroTik 相关 executor 命名。

      **主要变更**

      - 将 MikroTik 相关 executor 正式重命名为 `ros_address_list`，统一命名风格并贴近实际行为。
      - 修正文档中的功能描述和示例错误。
      - 补充格式化修正，保持代码与文档的一致性。

      **配置与升级说明**

      - 在 `v0.1.0` 中使用旧 MikroTik executor 名称的部署，升级到 `v0.1.1` 时需要同步调整配置中的插件类型名。
  </ReleaseCard>

  <ReleaseCard version="v0.1.0" badge="First Public Release" date="2026-03-28">
      **版本定位**

      - First Public Release，提供 OxiDNS 的首个公开版本和基础插件体系。

      **主要变更**

      - 建立 OxiDNS 的插件化主架构：`server -> DnsContext -> matcher / executor / provider -> upstream or side effects`。
      - 完成 UDP、TCP、DoT、DoQ、DoH 的 server 与 upstream 支持。
      - 提供与 MosDNS 风格接近的 `sequence` 编排、`jump/goto/return` 控制流和 `$tag` 引用方式。
      - 提供 `cache`、`forward`、`fallback`、`hosts`、`redirect`、`ecs_handler`、`dual_selector` 等核心 executor。
      - 提供 `domain_set`、`ip_set`、查询/响应条件、客户端 IP、响应 IP、CNAME 等 matcher / provider 能力。
      - 管理 API、健康检查、控制接口与插件相关 API 完成接入；CLI 增加 service-manager 集成。
      - 新增 Debian 打包、Docker 工作流和多平台 release 基础设施。
      - 建立 UDP/TCP/DoT/DoH/DoQ upstream 复用连接池，并优化 matcher、缓存、连接池、请求映射与时钟更新等热路径。

      **配置与升级说明**

      - Tokio worker 线程数可从配置调整，增强部署期可控性。
      - 提供 MikroTik RouterOS 动态路由与地址列表同步能力。
      - 支持 Linux 下 `ipset` / `nftset` 系统命令集成与测试覆盖。
      - 完成中英文 README、Quick Start、配置和模块文档的首轮建设。
  </ReleaseCard>
</div>
