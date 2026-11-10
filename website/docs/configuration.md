---
title: 配置总览
sidebar_position: 2
---

OxiDNS 使用 YAML 描述运行参数、管理接口、共享网络设置和插件执行链。本页用于选择正确的配置主题；完整可运行示例仍以仓库根目录的 `config.yaml` 为准。

## 最短配置流程

1. 从默认 `config.yaml` 或[场景配置](scenarios.md)开始，不要从空文件堆叠插件。
2. 根据[全局配置](configuration/global.md)设置工作线程、日志、网络出站、API 和插件实例。
3. 根据[执行链与控制流](configuration/sequence.md)组合 server、executor、matcher 和 provider。
4. 需要域名、IP 或外部数据集时，按[规则与 provider 语法](configuration/rules.md)编写规则。
5. 每次修改后先校验，再 reload 或重启：

```bash
oxidns check -c config.yaml
```

如果部署把配置与运行数据分开，校验时必须提供真实工作目录。例如 Debian 默认布局使用：

```bash
oxidns check -c /etc/oxidns/config.yaml -d /var/lib/oxidns
```

## 章节导航

| 主题 | 包含内容 |
| --- | --- |
| [全局配置](configuration/global.md) | 顶层字段、`include`、环境变量、tag、runtime、log、network、API、plugins |
| [执行链与控制流](configuration/sequence.md) | 四类插件职责、`sequence`、插件引用、quick setup、`accept` / `return` / `reject` / `jump` / `goto` |
| [规则与 provider 语法](configuration/rules.md) | 域名规则、IP/CIDR、`$tag` provider 引用和外部文件 |
| [插件参考](plugin-reference/overview.md) | 每种插件的完整参数、默认值、示例和平台限制 |

## 配置所有权

- 本章解释所有插件共享的配置模型，不重复维护各插件的字段表。
- 插件专属参数以[插件参考](plugin-reference/overview.md)为准。
- CLI 参数以[命令行工具](cli.md)为准，HTTP 结构以[管理 API](api.mdx)为准。
- 发布包中的 `config.yaml` 是当前版本的规范可运行示例；升级后应使用新二进制重新执行 `oxidns check`。

<span id="include"></span><span id="runtime"></span><span id="log"></span><span id="network"></span><span id="api"></span><span id="plugins"></span>

从旧版书签进入本页时，请使用上表跳转到拆分后的主题页。
