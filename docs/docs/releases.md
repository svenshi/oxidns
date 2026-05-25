---
title: 版本更新
sidebar_position: 4
---

import ReleaseCard from '@site/src/components/ReleaseCard';

# 版本更新

## 2026-05

<div className="release-stack">
   <ReleaseCard version="v1.1.1" badge="Patch Release" date="2026-05-25" defaultOpen>
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
