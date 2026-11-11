---
title: 命令行工具
sidebar_position: 3
---

OxiDNS 只有一个 `oxidns` 二进制。本页按任务导航命令；完整参数和行为说明位于三个专题页。

## 常用任务

| 目标 | 命令 | 参考 |
| --- | --- | --- |
| 校验配置 | `oxidns check -c config.yaml` | [配置与数据工具](cli/tools.md) |
| 前台启动 | `oxidns start -c config.yaml` | [运行、探测与系统服务](cli/runtime.md) |
| 临时调试日志 | `oxidns start -c config.yaml -l debug` | [运行、探测与系统服务](cli/runtime.md) |
| 探测上游 | `oxidns probe upstream tcp://1.1.1.1:53` | [运行、探测与系统服务](cli/runtime.md) |
| 安装系统服务 | `sudo oxidns service install -d /var/lib/oxidns -c /etc/oxidns/config.yaml` | [运行、探测与系统服务](cli/runtime.md) |
| 查看编译能力 | `oxidns build-info` | [配置与数据工具](cli/tools.md) |
| 导出 dat 规则 | `oxidns export-dat ...` | [配置与数据工具](cli/tools.md) |
| 检查或应用升级 | `oxidns upgrade check` / `oxidns upgrade apply` | [升级命令](cli/upgrade.md) |

## 帮助与退出码

使用 `oxidns --help` 查看顶层命令，使用 `oxidns <subcommand> --help` 查看当前二进制支持的完整参数。自动化流程应检查进程退出码：成功为 `0`，参数、校验或运行错误返回非零值。

```bash
oxidns --help
oxidns check --help
oxidns probe upstream --help
oxidns upgrade --help
```

<span id="start"></span><span id="check"></span><span id="probe"></span><span id="build-info"></span><span id="export-dat"></span><span id="service"></span><span id="upgrade"></span>

旧版章节书签会落到本入口页，请使用上表进入拆分后的命令说明。
