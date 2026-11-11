---
title: 社区、支持与贡献
---

OxiDNS 欢迎真实部署反馈、文档改进、问题修复和新能力提案。本页说明去哪里提问、如何准备问题，以及提交代码或文档时需要同步哪些内容。

## 选择正确的渠道

| 需求 | 渠道 |
| --- | --- |
| 使用问题、配置讨论、方案交流 | [GitHub Discussions](https://github.com/svenshi/oxidns/discussions) 或 [Telegram @OXIDNS](https://t.me/oxidns) |
| 可复现的软件缺陷 | [Bug Report](https://github.com/svenshi/oxidns/issues/new/choose) |
| 功能需求和使用场景 | [Feature Request](https://github.com/svenshi/oxidns/issues/new/choose) |
| 安全漏洞 | [私密安全报告](security.md#私密报告漏洞) |
| 已明确范围的代码或文档改进 | GitHub Pull Request |

公开问题中不要包含密码、token、私钥、私有域名、客户端 IP 或完整查询历史。

## 提交高质量问题

Bug 报告至少应包含：

- OxiDNS 版本、bundle、commit（如适用）。
- 操作系统、CPU 架构和安装方式。
- 脱敏后的最小配置与实际 `-d/--working-dir`。
- 可重复的查询、启动或 reload 步骤。
- 预期行为和实际行为。
- 第一条因果错误日志及已经运行的诊断命令。

先阅读[运维与故障排查](operations.md)，可以显著减少环境、端口、bundle 或工作目录问题造成的来回确认。

## 本地开发准备

项目使用 Rust 2024 edition。常用命令：

```bash
cargo check
cargo test
cargo test --test plugin_integration
```

仓库的 rustfmt 配置需要 nightly：

```bash
rustup toolchain install nightly
cargo +nightly fmt --all --check
cargo +nightly clippy --all-targets --all-features -- -D warnings
```

推荐在提交前运行：

```bash
just check
```

修改 Cargo feature、可选依赖或 bundle 时，还应运行 feature matrix；WebUI 和平台相关改动需要执行相应目录中的检查。

## 代码改动原则

- 保持核心请求路径：`server -> DnsContext -> matcher/executor/provider -> upstream 或 side effect`。
- 避免在每次查询中重复分配、解析、建连、加锁或执行阻塞 I/O。
- 配置解析、依赖分析和可复用状态尽量在初始化阶段完成。
- 修改 cache、fallback、rewrite 或合成响应时，覆盖 TTL、负缓存、RCODE 和 CNAME 等 DNS 语义。
- 网络测试使用临时端口、明确 timeout 和确定性输入。
- 新增系统副作用时，说明平台、权限、失败策略、资源上限和清理行为。

## 插件改动需要同步

新增、重命名或修改插件时，同一个 PR 通常需要同步：

1. Rust 插件工厂、feature 和 bundle。
2. `tests/plugin_integration.rs` 及相关单元测试。
3. 中英文插件参考。
4. WebUI 的 plugin definition 和中英文文本。
5. `README.md` / `README_EN.md`（能力或协议发生明显变化时）。
6. `config.yaml`（默认组合或必填字段变化时）。

文档站构建会检查中英文文件集合、插件总览与详细参考、Rust 注册表之间的一致性。

## 文档贡献

用户文档源文件位于：

- 中文：`website/docs/`
- 英文：`website/i18n/en/docusaurus-plugin-content-docs/current/`

行为、配置、API 和插件说明应同步更新两种语言。示例应使用描述性 tag，例如 `forward_main`、`cache_main`、`seq_main`，复杂 sequence 应把可复用逻辑提取成独立插件。

本地验证：

```bash
cd website
npm ci
npm run check:content
npm run build
```

构建会把断链和无效锚点视为错误。

## Pull Request 说明

PR 应明确写出：

- 改动目标和用户/运维影响。
- 配置、API、持久化、feature 和平台兼容性。
- 是否修改 DNS 请求热路径。
- 已同步的代码、WebUI、文档和配置表示。
- 实际运行的验证命令。
- 尚未验证的平台或环境假设。

提交信息使用 Conventional Commits，例如 `feat(cache): add negative cache persistence`。保持改动聚焦，不在同一 PR 中顺手重构无关模块。

## 社区协作

请围绕技术事实、可复现结果和实际使用场景讨论。对不同部署选择保持尊重；提出反对意见时说明约束、风险和可验证的替代方案。
