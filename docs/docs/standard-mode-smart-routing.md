---
title: 标准模式智能分流
sidebar_position: 4
---

标准模式 schema v5 把国内、远程和未知域名策略编译为 OxiDNS 原生的 provider、matcher、sequence、forward、fallback 与 cache 插件图。它不读取或修改 OpenWrt/UCI、操作系统 DNS、DHCP、防火墙以及任何第三方代理系统。

## 能力边界

“上游防泄漏”只约束已经到达 OxiDNS 的查询。严格远程模式保证未知域名不会执行国内或默认上游，但无法拦截绕过 OxiDNS 的应用、硬编码 IP、或发往其他解析器的加密 DNS。`outbound` 和 SOCKS 只是 OxiDNS 自身的网络出口参数，不代表对第三方系统的控制。

## 语义数据角色

| 角色 | 用途 |
| --- | --- |
| `domestic_domains` | 将域名送入国内路径 |
| `foreign_domains` | 将域名送入远程路径 |
| `domestic_ips` | 校验国内路径返回的 A/AAAA 地址 |
| `direct_domains` | 显式使用本机的国内/直连 DNS 路径 |
| `remote_domains` | 显式使用远程 DNS 路径 |
| `ddns_domains` | 使用短 TTL，并绕过缓存 |

每个角色可以组合手工规则、本地文本文件、在线订阅、原生 `geosite.dat`/`geoip.dat` 四种数据源。OxiDNS 不预设订阅地址、文件名或国家数据；本地文件必须已经存在，订阅文件由标准模式放入自己的数据目录。

在线订阅会生成独立的下载、定时任务和 Provider reload 链。下载失败时不会用失败内容覆盖最后一次成功文件，也不会 reload 当前 Provider。WebUI 的“规则路由”页会显示尚未应用、文件缺失、文件过期、下载失败、装载失败和规则数量，并支持单源立即刷新。

## 三种未知域名模式

| 模式 | 初始路径 | 回退 | 缓存边界 |
| --- | --- | --- | --- |
| 兼容优先 | 国内 | 远程 | `unknown_compatibility` 独立命名空间 |
| 隐私优先 | 远程 | 仅在显式允许时回退国内 | `unknown_privacy` 独立命名空间 |
| 严格远程 | 远程 | 不执行国内或默认路径 | `unknown_strict_remote` 独立命名空间 |

国内和远程路径必须不同。模式切换、响应 fallback 和语义路径均使用独立缓存插件，避免把不同解析策略下的结果交叉复用。

## 国内响应校验

对 A/AAAA 查询，国内路径使用 `resp_ip` 和 `domestic_ips` Provider 校验地址。以下结果可以独立配置，默认都会显式清除国内响应并进入远程 fallback：

- 地址不属于国内 IP 集合：`domestic_ip_mismatch`；
- 只有 CNAME：`cname_only`；
- NOERROR 但没有期望记录：`nodata`；
- NXDOMAIN：`nxdomain`；
- SERVFAIL：`servfail`；
- 超过路径阈值：`timeout`；
- 上游执行或网络错误：`transport_failure`。

有效国内地址直接接受。非 A/AAAA 查询不会进行 IP 地理校验。启用查询记录后，详情页会显示语义角色、初始路径、校验结果、fallback 原因、选中的分支、最终路径和最终上游组。

## ECS、双栈与 IP 优选

每条路径可以独立设置：

- ECS：继承、移除、保留客户端 ECS、按客户端地址生成、或使用固定 preset；
- 双栈：禁用偏好、优先 IPv4、优先 IPv6、仅 IPv4、仅 IPv6；
- IP 优选：`first_success`、`best_within_budget`、`background`，并限制探测方法、并发、等待预算和缓存；
- DNSSEC：只允许 `reorder_only` 或 `skip`，不提供会删除已签名 RRset 成员的模式。

ECS 在缓存之前处理。会携带或生成 ECS 的路径自动启用 `ecs_in_key`。IPv4-only/IPv6-only 是查询类型阻断规则，不等同于地址偏好；`ip_selector` 也独立于上游竞速和双栈抑制。

## 应用前检查

保存时 Rust 后端仍是唯一权威：它会迁移和规范化意图、检查构建能力与引用、验证数据文件、显示重复/被覆盖/不可达规则，生成候选配置并执行后端预检。确认后才通过可恢复事务应用。schema v4 会迁移为 v5；旧的 ECS 和 IP 优选占位值会给出复核警告，不会静默扩大路由范围。

完整接口和事务字段见[标准模式 Plan/Apply](api/standard-mode.mdx)。
