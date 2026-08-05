# 标准/专家模式后端反耦合交接

## 1. 结论

本轮已把 Standard/Expert 产品控制面完整收回 WebUI。后端只保留原生 YAML 校验与事务、构建/插件能力、透明 WebUI JSON 和原始查询事件；Cargo `standard` 仍然只是构建套装，不表达产品模式。

最终链路：

```text
WebUI Intent
  -> TypeScript migrate / normalize / validate / compile / plan / explain
  -> native OxiDNS YAML
  -> generic /config/validate
  -> generic /config/apply
  -> DNS runtime
  -> raw bounded query events
  -> WebUI query-explainer
```

## 2. 删除范围

已删除：

- `src/api/standard_mode.rs`；
- `src/config/standard_mode/` 全目录；
- `tests/standard_mode_integration.rs`；
- `/api/standard/*` 全部路由及前端调用；
- 后端 Standard Intent、Plan、模板、Expert Copy/Analysis、所有权、模式专属事务/历史/资产；
- Query Recorder 后端 `diagnosis` 聚合和 `intentRevision` 产品解析。

上述模式专属 Rust 与集成测试共删除 13,345 行。新的原生 YAML 集成 fixture 不再引用任何产品模式类型。

## 3. 新增通用后端能力

### 配置事务

- `POST /api/config/validate` 在真实配置目录旁校验 include 和相对路径，并返回候选版本与依赖图；
- `POST /api/config/apply` 使用磁盘基础版本和候选版本双 CAS，正文上限 2 MiB；
- `GET /api/config/apply/status` 提供通用事务状态；
- `GET /api/config/history` 提供最多 20 条、YAML 累计最多 20 MiB 的健康历史；
- `POST /api/config/history/restore` 只返回 YAML 预览；
- `PUT /api/config` 的 save-only 使用原子替换，reload 路径复用同一事务；
- 所有 YAML 写入共享通用锁；事务 ID 为 `config-{timestamp}-{pid}-{hash12}`；
- 边车文件固定为 `.config-transaction.json`、`.config-transaction.last.json`、`.config-history.json`；
- 启动恢复 pending journal；reload/组装/必要收尾失败恢复真实健康 YAML 和上一运行时；
- 健康历史写入是非关键附加步骤，失败只告警。

`AppController` 持有当前健康运行时对应的 YAML 原文及版本。因此“只保存、未 reload”以后再 Apply，失败回滚目标仍是实际运行版本，而不是磁盘上的中间版本。

### 透明状态与查询轨迹

- `/api/webui/config` 默认值为 `{"schema":1}`，容量 2 MiB，锁与 DNS 配置事务独立；
- 后端不解析工作区里的 Standard/Expert 字段；
- Query Recorder 继续保留有界 steps、时间、详情、截断统计、SQLite 增量迁移和任意字符串 `context`；
- 查询详情只返回 Record、Steps 与 Opaque Context，不返回产品 `diagnosis`。

## 4. WebUI 唯一控制面

- 新 TypeScript 编译器覆盖上游组和全部策略、路径级 Cache/ECS/双栈/IP 优选、过滤与订阅、本地策略、智能分流、专属组/监听、动态学习、高级规则、稳定 tag/优先级/路径边界和能力诊断；
- 使用 `yaml` 生成确定性 YAML，使用 Web Crypto 对规范化 Intent 计算 SHA-256；
- 保留基础配置中的 `include`、`api`、`network`、`runtime`、`log` 和非托管字段；
- 19 组旧 Rust 编译器黄金样本覆盖默认、五种上游策略、Cache/ECS、过滤/本地、智能路由、专属监听、动态学习、高级规则、schema 1～6 和能力缺失；
- 模板存 IndexedDB，按 OxiDNS 实例隔离，最多 64 条、累计 2 MiB；
- Intent 导入导出、模板预览、Expert Copy 和 Expert Analysis 都是本地操作；Expert Copy 只填充编辑器；
- DNS Apply 成功后工作区 JSON 使用 CAS 最多重试三次。工作区失败不回滚 DNS；UI 保留部分成功状态和 Intent 下载恢复路径；
- Query Explainer 只有 revision 与活动映射一致时才解释，否则显示原始事实。

## 5. 兼容与迁移限制

- 不迁移旧 `.standard-transaction`、`.standard-history`、`.standard-assets` 边车文件；
- 已存在的透明 WebUI JSON 仍由新版前端读取并规范化；
- 动态学习配置被删除后不自动删除旧规则/元数据文件；
- Standard 页面路由 `/standard/*` 保留，它只是浏览器路由；后端 `/api/standard/*` 均为 404；
- 生成 YAML 中的前端注释与 Query Recorder context 对后端完全不透明；
- IndexedDB 模板不会跨浏览器自动同步，需显式导入导出；
- `standard` Cargo feature/bundle 名称保持不变且不计入后端产品耦合。

## 6. 验收记录

已通过：

- `just check`：1,112 个默认 Rust 单元测试、4 个 feature-gating 测试、1 个原生策略测试和 89 个插件集成测试；
- `just check-matrix`：42 个公开 feature 独立编译，minimal 662、standard 997、all-features 1,112 个单元测试，以及 minimal/standard/full Clippy 和对应集成测试；
- Windows GNU Standard 与 Linux musl Standard 交叉 `cargo check`；
- WebUI `typecheck`、无警告 lint、20 个 Vitest 文件/128 个测试、生产构建；
- 中英文 Docusaurus 内容检查与生产构建；
- 本机无凭据隔离脚本：Standard bundle、旧路由 404、透明状态、真实目录 Validate、伪造版本拒绝、原子 Apply、真实 DNS 查询、只读历史预览、运行时组装失败回滚、通用边车名称全部通过；
- 隔离机 `root@172.16.2.55`：同一无凭据脚本在 `/tmp/oxidns-standard-decoupling.67da42858633/` 执行通过，覆盖 Standard bundle、旧路由 404、透明状态、真实目录 Validate、原子 Apply、真实 DNS 查询、只读历史预览、运行时组装失败回滚和通用边车名称；
- Linux x86_64 musl Standard release：static PIE、stripped，SHA-256 `67da42858633baf3476fc26fba663a88456d1151e5f02b6005cb2f15f8d4d87a`；
- 后端反耦合扫描无 `standard_mode`、`StandardIntent`、`StandardPlan`、`ExpertCopy`、模式所有权或 `/api/standard`；
- 两份冻结规划相对 `90a5dec` 零字节差异。

第一次在受限沙箱执行 bundle 网络测试时，16 个 loopback bind 测试因 `Operation not permitted` 失败；按测试策略在允许本地临时端口的相同矩阵中重跑后全部通过。这是环境分类，不是产品失败。

最终 Linux musl 二进制与无凭据脚本保留在隔离机 `/tmp/oxidns-standard-decoupling.67da42858633/`，远端 SHA-256 与本地制品一致。

## 7. 交接文件

- 开发计划：`development/standard-mode/backend-decoupling-plan.md`
- 本交接：`development/standard-mode/backend-decoupling-handoff.md`
- 无凭据隔离验收：`development/standard-mode/backend-decoupling-isolation.py`
- Rust 黄金样本：`webui/lib/standard-mode/fixtures/rust-golden.json`
- 原生策略 fixture：`tests/fixtures/native_policy.yaml`
