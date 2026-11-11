---
title: 文档版本与维护
---

# 文档版本与维护

## 当前文档对应什么

oxidns.org 的默认手册跟随 OxiDNS 仓库 `main` 分支，Docusaurus 内部标记为 `current` / `Next`。因此它描述的是当前主线能力，可能包含尚未进入你所安装 release 的字段、插件或行为。

项目目前没有在站点上维护每个 release 的冻结文档快照。需要复现某个历史版本时，应切换到对应 Git tag 阅读仓库中的 `README.md`、`website/`、`config.yaml` 和 release note。

## 确认部署能力

不要仅根据网页、archive 名称或镜像标签推断本机能力。按以下顺序确认：

```bash
oxidns --version
oxidns build-info
oxidns check -c /path/to/config.yaml -d /working/directory
```

- `--version` 确认 release。
- `build-info` 确认 bundle、Cargo features、协议和已编译插件。
- `check` 确认该二进制能读取真实配置和工作目录中的资源。
- [版本更新](releases.md)记录行为变化、迁移要求和发布范围。

## 内容来源与更新责任

| 内容 | 规范来源 | 同步要求 |
| --- | --- | --- |
| 可运行默认配置 | 仓库根目录 `config.yaml` | 字段、插件或默认值变化时同步手册和 WebUI 定义 |
| 插件类型与编译能力 | Rust 插件注册表和 Cargo features | 插件总览、分类目录和中英文正文由内容检查共同约束 |
| CLI / API 行为 | 当前二进制实现 | 接口变更同时更新专题页、示例和迁移说明 |
| 发布历史 | Git tag 与 release note | 已完成事项只进入版本更新，不继续堆积在路线图 |
| 内部维护流程 | `docs/ai/` 与 `AGENTS.md` | 不作为最终用户手册发布 |

文档问题可以按[贡献指南](contributing.md)提交。报告时请附 OxiDNS 版本、bundle、目标平台、问题页面和可复现配置片段，并移除凭据、私有域名和客户端数据。
