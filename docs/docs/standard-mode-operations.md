---
title: 标准模式解释、诊断与配置资产
---

标准模式把意图、生成的原生 YAML 和单次查询用稳定修订号连接起来。全部产品解释发生在 WebUI；OxiDNS 后端只处理原生配置、通用事务和原始运行事件。标准模式不修改 OpenWrt/UCI、操作系统 DNS、DHCP、防火墙或第三方控制器。

## 浏览器编译与解释

TypeScript 编译器会先把 schema 1～6 意图迁移并规范化，再生成：

- 基于规范化意图的 SHA-256 `intentRevision`；
- 稳定对象 ID 到 Provider、Matcher、Path、Cache、Upstream 与 Listener tag 的映射；
- 最终优先级、路径边界、能力诊断、依赖图与语义 Diff；
- 确定性的原生 OxiDNS YAML。

编译器从 `/api/build` 获取当前构建能力。浏览器检查负责产品反馈，通用 `/api/config/validate` 对候选原生 YAML、include 和相对路径执行最终校验。Apply 只接受通过版本 CAS 的 YAML，不接收 Intent 或 Plan。

## 有界查询轨迹与前端解释

Standard 生成的 `query_recorder` 可把 `intentRevision` 和生成上下文作为普通字符串映射写入记录。每次查询默认最多 512 个事件，Expert YAML 可配置 32～4096。超限时后端返回 `steps_truncated` 和 `dropped_step_count`。

后端只返回 Cache、Forward、Fallback 等原始步骤、时间与截断事实，不生成 `diagnosis`，也不解析 revision。WebUI 仅在记录 revision 与活动 Intent/生成映射匹配时添加规则、路径、缓存和上游解释；不匹配时降级为原始事实，避免用当前工作区猜测旧记录。

旧 query-recorder SQLite 数据库会增量迁移继续读取。事件仍受单请求上限、上游 fan-out、记录队列、保留期和数据库读取并发约束，且不得记录上游凭据。

## 配置资产与恢复

- Intent 导入导出在浏览器本地完成，导入后仍需编译、Diff、通用校验和 Apply。
- 模板存入浏览器 IndexedDB，按 OxiDNS 实例隔离，最多 64 条、累计最多 2 MiB。
- “复制为专家配置”只把生成 YAML 放入 Expert 编辑器，不修改磁盘或运行时。
- Expert Analysis 调用通用 `/config/validate`，产品分类由前端插件目录完成。
- 活动 Intent、生成映射和最近生成信息作为透明 JSON 保存在 `/api/webui/config`。DNS 生效后工作区 CAS 最多重试三次；仍失败时不回滚 DNS，并提供 Intent 下载。
- 后端健康 YAML 历史最多 20 条、正文累计最多 20 MiB。Restore 只返回 YAML 预览，必须重新走 Validate 和 Apply。
- 删除动态学习配置不会自动删除旧规则或元数据文件；WebUI 提示残留，后端不提供模式专属文件删除接口。

模式边界见[标准模式与后端边界](api/standard-mode.mdx)，事务字段见[配置接口](api/configuration.mdx)。
