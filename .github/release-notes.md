# OxiDNS v1.8.0

## 🚀 发布概览

- v1.8.0 是功能版本，扩展策略编排、EDNS 过滤、域名学习、缓存管理和查询分析，并修复 ECS 来源选择、持久化回滚、DNS 协议处理与 RouterOS 联动。

## ✨ 主要亮点

- 新增 `load_balance` 执行器，支持轮询、随机和加权随机；新增 `has_cname` matcher，两者均可用于 minimal bundle。
- 新增 minimal 可用的 `edns_filter` 执行器，支持 `preserve`、`strip`、`strip_unknown` 策略及 option code 白名单/黑名单；默认在后续执行链退出或取消时恢复整个请求。补齐 EDNS 注册代码识别，未实现 payload 解析的选项仍保留原始数据，查询记录同步显示名称。
- 修复 `ecs_handler` 删除客户端 ECS 后未继续应用 `preset`/`send` 的问题；非 IN 查询也遵循删除策略，但不生成 ECS。
- `hosts.format: standard` 支持标准 hosts 文件与地址合并；`learn_domain.rule_kind: registrable_domain` 使用内置公共后缀表学习可注册域及其子域。
- WebUI 查询分析增加统一时间条、自适应趋势桶、统计截断提示和记录下钻，支持精确域名、客户端 IP 与特殊响应分类筛选；补充 EDNS 过滤配置入口并修复 YAML 嵌套字段补全。
- 缓存支持按 answer IP 过滤和批量删除；启动时等待恢复，失败的启动/reload 不再覆盖历史 dump，读取失败时保护原文件。
- RouterOS 路由增加可选 `scope`、`target_scope`，修复地址列表重复插入恢复、地址表示及清理。
- 修正 NOTIMP/BADVERS 响应、超大 TCP 帧、TCP keepalive 和 DoQ/DoH3 停机任务等待；支持 OpenWrt `/etc/TZ`，同步 zoneparser 修复。
- 管理 API 限制请求体大小、改进 Basic Auth 凭据比较；配置原子保存保留 Unix 权限、属主、属组及符号链接。
- Docker 新增 ARMv7 镜像；文档源码迁至 `website/`，补充社区 WebUI 推荐。`oxidns-proto` 与 `oxidns-macros` 改为 MIT/Apache-2.0 双许可证，主项目许可证不变。

## ⚠️ 升级说明

- **统计 API 调用变更**：`query_recorder` 的 `/stats/*` 必须提供 `since_ms`、`until_ms`，开始时间早于结束时间，跨度最多 24 小时；旧的不带时间调用会返回 HTTP 400。统计最多使用区间内最近 10,000 条记录；列表、详情与 SSE 接口保持原有时间参数语义。
- **回退行为变更**：显式 `fallback.threshold: 0` 现在立即启动备用分支；如需保留旧延迟，改为 `500`。省略字段仍默认 500 毫秒。
- **ECS 行为修复**：IN 查询依次采用保留的客户端 ECS、`preset`、`send` 对应的客户端地址；`forward: false` 删除原 ECS 后仍可补发。非法 quick-setup IP 现在会导致配置加载失败；旧 `IP/mask` 写法仍忽略 mask，使用 IPv4 `/24`、IPv6 `/48`，自定义前缀请用完整配置。
- 除上述行为修复和校验收紧外，现有 v1.6.1 YAML 配置可继续使用，新增字段均为可选。`edns_filter` 需要显式加入执行链，默认 `policy: preserve`、`restore_request: true`，白名单和黑名单默认留空。管理 API 请求体上限为 4 MiB，大型配置提交或缓存导入需核对大小；建议同步更新随包 WebUI。
- 从 v1.6.1 升级无需迁移查询历史或重新安装 Windows 服务；更早版本仍须遵循 v1.6.0 的迁移说明。升级前备份配置和持久化数据，并运行 `oxidns check -c <配置文件>`。
- 根 crate 为 `1.7.0`，tag 为 `v1.7.0`；辅助 crate 为 `oxidns-proto 0.1.7`、`oxidns-macros 0.1.1`、`oxidns-zoneparser 0.1.3`，`oxidns-ripset` 保持 `0.1.3`。

## 📦 下载与校验

- 根据平台和 bundle 选择对应 archive，并使用 GitHub Release asset 的 SHA256 digest 校验下载文件。

## ✅ 验证

- `just check`
- `just check-matrix`
- EDNS 补充验证：`cargo test --locked -p oxidns-proto`、`cargo test --locked --no-default-features --features minimal edns_filter`。
- WebUI：`pnpm typecheck`、`pnpm lint`、`pnpm test`、`pnpm build`
- 双语文档：`npm run build`
- Telegram 发布消息：`python3 -m unittest discover -s .github/scripts -p '*_test.py'`，渲染后无需截断。
- full/minimal：`oxidns --version`、`oxidns build-info` 与对应的 `oxidns check -c config.yaml` / `oxidns check -c config.minimal.yaml`。
- crates.io workspace dry-run：`cargo publish --locked --dry-run --allow-dirty --workspace`；实际发布由 Action 先发布辅助 crate，再验证和发布根包。
