---
title: 插件总览
sidebar_position: 1
---

OxiDNS 的所有能力都来自插件，按职责分为四层：

- `server`：网络入口，负责监听与接入协议。
- `executor`：执行动作，负责转发、缓存、重写、观测和系统联动。
- `matcher`：条件判断，负责给 `sequence` 提供策略分支条件。
- `provider`：数据提供，沉淀可复用的域名 / IP 规则集。

复杂策略通常由多类插件组合实现，典型组合如下：

```text
server -> sequence
  -> matcher 判断
  -> executor 执行
  -> provider 提供规则集
  -> upstream 或 side effect
```

下面列出全部内置插件，可直接点击插件名跳转到字段说明。

## 服务端插件（server）

详细字段见 [服务端插件](server.md)。

| 插件 | 作用 |
| --- | --- |
| [`udp_server`](server.md#udp_server) | 监听 UDP DNS 请求，并把请求转交给入口执行器。 |
| [`tcp_server`](server.md#tcp_server) | 监听 TCP DNS 请求；同时配置证书与私钥时可作为 DoT 入口。 |
| [`http_server`](server.md#http_server) | 提供 DNS over HTTPS（DoH），可同时支持 HTTP/2 与可选 HTTP/3。 |
| [`quic_server`](server.md#quic_server) | 提供 DNS over QUIC（DoQ）。 |

## 执行器插件（executor）

详细字段见 [执行器插件](executor.md)。按 “策略编排 → 请求处理 → 响应改写 → 观测调试 → 副作用联动 → 维护任务” 的顺序分组。

### 策略编排

| 插件 | 作用 |
| --- | --- |
| [`sequence`](executor.md#sequence) | 把多个 matcher 和 executor 编排成一条流水线，是最常用的入口执行器。 |
| [`fallback`](executor.md#fallback) | 在主路径失败或过慢时，切换到备用路径。 |

### 请求处理

| 插件 | 作用 |
| --- | --- |
| [`forward`](executor.md#forward) | 向上游发起 DNS 查询。 |
| [`cache`](executor.md#cache) | 对响应做 TTL 感知缓存，支持负缓存与持久化。 |
| [`hosts`](executor.md#hosts) | 按域名规则直接返回静态 `A` / `AAAA`。 |
| [`arbitrary`](executor.md#arbitrary) | 加载任意静态 DNS 记录并在命中时直接构造应答。 |
| [`redirect`](executor.md#redirect) | 把请求域名改写为另一个目标域名，并在返回阶段补回客户端可见的 CNAME。 |
| [`ecs_handler`](executor.md#ecs_handler) | 处理 EDNS Client Subnet：保留、改写或按来源 IP 自动补齐。 |
| [`forward_edns0opt`](executor.md#forward_edns0opt) | 把指定 EDNS0 option code 从请求转发到最终响应中。 |

### 响应改写

| 插件 | 作用 |
| --- | --- |
| [`ttl`](executor.md#ttl) | 改写响应 TTL（统一值或上下限裁剪）。 |
| [`prefer_ipv4` / `prefer_ipv6`](executor.md#prefer_ipv4--prefer_ipv6) | 双栈优选器，对偏好类型做学习，对非偏好类型做抑制。 |
| [`black_hole`](executor.md#black_hole) | 对命中的 `A` / `AAAA` 请求直接返回预设地址。 |
| [`drop_resp`](executor.md#drop_resp) | 清空当前上下文中的响应。 |
| [`reverse_lookup`](executor.md#reverse_lookup) | 缓存应答中的 IP → 域名关系，并可选地处理 PTR 查询。 |

### 观测与调试

| 插件 | 作用 |
| --- | --- |
| [`query_summary`](executor.md#query_summary) | 在后续链路执行完后输出紧凑查询摘要。 |
| [`query_recorder`](executor.md#query_recorder) | 把请求、响应、`sequence` 路径事件持久化到 SQLite，并暴露历史查询、统计和 SSE 实时推送。 |
| [`metrics_collector`](executor.md#metrics_collector) | 收集轻量级请求计数与延时指标，并导出 Prometheus 格式。 |
| [`debug_print`](executor.md#debug_print) | 打印请求与响应对象，便于调试。 |
| [`sleep`](executor.md#sleep) | 异步延迟，用于测试和策略实验。 |

### 副作用与系统联动

| 插件 | 作用 |
| --- | --- |
| [`http_request`](executor.md#http_request) | 向外部 `http/https` 服务发送回调请求，适合 webhook、审计、告警和外部联动。 |
| [`script`](executor.md#script) | 执行外部命令，并把 `DnsContext` 中的稳定字段注入为参数或环境变量。 |
| [`ipset`](executor.md#ipset) | 把响应中的 IP 写入 Linux `ipset`（内置 netlink 后端，无需 `ipset` 命令）。 |
| [`nftset`](executor.md#nftset) | 把响应 IP 写入 Linux `nftables set`（内置 netlink 后端，无需 `nft` 命令）。 |
| [`ros_address_list`](executor.md#ros_address_list) | 把应答 IP 同步到 RouterOS `address-list`，支持动态项、常驻项和关闭清理。 |

### 维护与调度

| 插件 | 作用 |
| --- | --- |
| [`upgrade`](executor.md#upgrade) | 在执行器链路中触发 OxiDNS 升级流程。 |
| [`download`](executor.md#download) | 下载一个或多个 `http/https` 文件到本地，并在写入完成后原子覆盖。 |
| [`reload_provider`](executor.md#reload_provider) | 按 tag 定向重建 provider 内部快照，不触发整体配置 reload。 |
| [`reload`](executor.md#reload) | 触发与 `POST /reload` 相同的应用级全量 reload。 |
| [`cron`](executor.md#cron) | 后台调度一组 executor，按 cron 表达式或固定间隔触发任务。 |

## 匹配器插件（matcher）

详细字段见 [匹配器插件](matcher.md)。

### 请求维度

| 插件 | 作用 |
| --- | --- |
| [`qname`](matcher.md#qname) | 匹配请求中的查询域名。 |
| [`question`](matcher.md#question) | 按 provider 的 `contains_question` 语义匹配请求中的 question。 |
| [`qtype`](matcher.md#qtype) | 匹配请求中的 qtype。 |
| [`qclass`](matcher.md#qclass) | 匹配请求中的 qclass。 |
| [`client_ip`](matcher.md#client_ip) | 匹配客户端来源 IP。 |
| [`ptr_ip`](matcher.md#ptr_ip) | 从 PTR 请求名中解析 IP 并做匹配。 |

### 响应维度

| 插件 | 作用 |
| --- | --- |
| [`resp_ip`](matcher.md#resp_ip) | 匹配响应 answers 中的 A / AAAA IP。 |
| [`cname`](matcher.md#cname) | 匹配响应中的 CNAME 目标域名。 |
| [`rcode`](matcher.md#rcode) | 匹配当前响应的 rcode。 |
| [`has_resp`](matcher.md#has_resp) | 只要上下文中已有响应就命中。 |
| [`has_wanted_ans`](matcher.md#has_wanted_ans) | 判断响应 answers 中是否包含与请求 qtype 对应的记录。 |

### 上下文与表达式

| 插件 | 作用 |
| --- | --- |
| [`mark`](matcher.md#mark) | 匹配上下文中的 mark 集合。 |
| [`env`](matcher.md#env) | 匹配进程环境变量。 |
| [`random`](matcher.md#random) | 按概率命中，适合灰度和采样。 |
| [`rate_limiter`](matcher.md#rate_limiter) | 基于客户端 IP 的令牌桶限流。 |
| [`string_exp`](matcher.md#string_exp) | 通用字符串表达式匹配器，补足专用 matcher 不够灵活的场景。 |

### 组合与常量

| 插件 | 作用 |
| --- | --- |
| [`any_match`](matcher.md#any_match) | 组合多个 matcher 表达式，任意一个命中即返回 `true`。 |
| [`_true`](matcher.md#_true) | 恒为真。 |
| [`_false`](matcher.md#_false) | 恒为假。 |

## 数据提供器插件（provider）

详细字段见 [数据提供器插件](provider.md)。

| 插件 | 作用 |
| --- | --- |
| [`domain_set`](provider.md#domain_set) | 高性能域名规则集合，可被 `qname`、`cname` 等插件引用。 |
| [`geosite`](provider.md#geosite) | 从 v2ray-rules-dat 的 `geosite.dat` 中提取一个或多个 code，并编译成可复用域名规则集合。 |
| [`adguard_rule`](provider.md#adguard_rule) | 提供 AdGuard Home DNS 规则子集的可复用 provider。 |
| [`ip_set`](provider.md#ip_set) | IP / CIDR 规则集合，可被 `client_ip`、`resp_ip`、`ptr_ip` 等 matcher 引用。 |
| [`geoip`](provider.md#geoip) | 从 v2ray-rules-dat 的 `geoip.dat` 中提取一个或多个 code，并编译成可复用 IP / CIDR 集合。 |
