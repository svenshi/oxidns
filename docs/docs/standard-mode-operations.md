---
title: 标准模式解释、诊断与配置资产
---

标准模式把“意图、运行时图、单次查询和历史版本”用稳定修订号连接起来。它仍然只管理 OxiDNS 自身配置和数据文件，不会修改 OpenWrt/UCI、操作系统 DNS、DHCP、防火墙、`ipset`、`nftset`、RouterOS 或第三方控制器。

## 编译解释

每次 Plan 都会返回 schema 1 的 `plan.generated.explanation`：

- `intentRevision` 是规范化 schema v6 意图的 SHA-256；
- `mappings` 把稳定对象 ID 映射到 Provider、Matcher、Path、Cache、Upstream 和 Listener tag；
- `finalPriority` 是最终有效顺序，而不是表单显示顺序；
- `pathBoundaries` 明确每条路径的上游组、成员、缓存命名空间、ECS 是否进入缓存键、过滤、日志、双栈和 IP 优选行为；
- `capabilities` 列出当前构建拥有及缺少的可选能力；
- Plan 顶层 `dependency_graph` 提供插件节点、依赖边、初始化顺序和 sequence 流程；生成 YAML 始终只读，Apply 仍是唯一激活入口。

`semantic_diff` schema 1 按稳定对象比较最近成功意图和候选意图，并报告受影响的路径、规则、缓存、监听器、上游组和管理文件。当前配置不是可信 Standard 版本时，结果明确标记为 `takeover`，不会伪造精确基线。

## 有界单查询诊断

Standard 生成的查询记录器把 `intentRevision` 固定到每条记录。一次查询最多记录 512 个事件；Expert 可配置 32–4096，超过上限只增加该查询的丢弃计数。记录会显示 `stepsTruncated` 和 `droppedStepCount`，不会把不完整轨迹误称为完整证据。

诊断事件覆盖：

- 第一条未命中的 matcher 和默认路径原因；
- 缓存 fresh/stale/miss/expired、ECS 是否进入缓存键；
- fallback 分支和原因；
- 稳定上游成员 ID、选择、未选响应、超时、传输错误和选中后取消；
- 最终 RCODE、响应来源、总耗时与已有阶段耗时。

事件只在请求已开启录制时生成，并受请求事件数、上游最大 fan-out、记录队列、保留期和数据库读取并发限制。OxiDNS 不为域名、客户端、意图修订号或单查询结果创建高基数指标，也不会把上游凭据写入事件。

旧 query-recorder v1 数据库会以增量列迁移继续读取。旧记录没有意图修订号时，API 返回原始事实并标记 `explanationUnavailable`，不会用当前意图猜测旧查询。

## 配置资产与回滚

- 导出使用 `oxidns_standard_intent`、asset schema 1 封装完整规范化意图、来源 schema、构建信息和稳定修订号。
- 导入大小上限为 2 MiB，先走现有 schema 迁移、规范化、校验、语义 Diff 和 Plan；它本身不写配置，确认后仍走精确版本 Apply。
- 最近 20 个成功版本保存在配置旁的原子 `0600` 历史文件中。恢复先返回意图，再重新 Plan/Apply；失败候选不能替换上一健康运行时。
- “复制为专家配置”只返回分离的 YAML、版本和依赖图，不切换模式、不写文件，也不继续宣称 Standard 所有权。
- Expert 分析只解析、校验并生成依赖图，指出 Expert-only 和系统集成插件；它不运行配置、不探测外部系统，也不尝试把任意插件图有损反编译为 Standard 意图。
- 保存模板位于配置旁的本地 asset schema 1 文件中，最多 64 条、总计 2 MiB、原子 `0600` 写入并使用乐观版本。保存、复制和删除不会修改运行时；使用前仍通过服务端模板预览检查碰撞。

完整字段与路由见[标准模式 Plan/Apply API](api/standard-mode.mdx)。
