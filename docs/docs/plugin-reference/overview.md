---
title: 插件总览
sidebar_position: 1
format: mdx
---

import PluginAvailability, {BundleGuide} from "@site/src/components/PluginAvailability";

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

<BundleGuide />

## 服务端插件（server）

详细字段见 [服务端插件](server.mdx)。

<div className="plugin-catalog">

| 插件 | 作用 | 内置版本 |
| --- | --- | --- |
| [`udp_server`](server/datagram-stream.mdx#udp_server) | 监听 UDP DNS 请求，并把请求转交给入口执行器。 | <PluginAvailability from="minimal" compact /> |
| [`tcp_server`](server/datagram-stream.mdx#tcp_server) | 监听 TCP DNS 请求；同时配置证书与私钥时可作为 DoT 入口。 | <PluginAvailability from="minimal" compact /> |
| [`http_server`](server/encrypted-http.mdx#http_server) | 提供 DNS over HTTPS（DoH），可同时支持 HTTP/1.1、HTTP/2 与可选 HTTP/3。 | <PluginAvailability from="standard" compact /> |
| [`quic_server`](server/encrypted-http.mdx#quic_server) | 提供 DNS over QUIC（DoQ）。 | <PluginAvailability from="standard" compact /> |

</div>

## 执行器插件（executor）

详细字段见 [执行器插件](executor.mdx)。按 “策略编排 → 请求处理 → 响应改写 → 观测调试 → 副作用联动 → 维护任务” 的顺序分组。

### 策略编排

<div className="plugin-catalog">

| 插件 | 作用 | 内置版本 |
| --- | --- | --- |
| [`sequence`](executor/control-flow.mdx#sequence) | 把多个 matcher 和 executor 编排成一条流水线，是最常用的入口执行器。 | <PluginAvailability from="minimal" compact /> |
| [`fallback`](executor/control-flow.mdx#fallback) | 在主路径失败或过慢时，切换到备用路径。 | <PluginAvailability from="minimal" compact /> |

</div>

### 请求处理

<div className="plugin-catalog">

| 插件 | 作用 | 内置版本 |
| --- | --- | --- |
| [`forward`](executor/resolution.mdx#forward) | 向上游发起 DNS 查询。 | <PluginAvailability from="minimal" compact /> |
| [`cache`](executor/resolution.mdx#cache) | 对响应做 TTL 感知缓存，支持负缓存与持久化。 | <PluginAvailability from="minimal" compact /> |
| [`hosts`](executor/resolution.mdx#hosts) | 按域名规则直接返回静态 `A` / `AAAA`。 | <PluginAvailability from="minimal" compact /> |
| [`arbitrary`](executor/resolution.mdx#arbitrary) | 加载任意静态 DNS 记录并在命中时直接构造应答。 | <PluginAvailability from="standard" compact /> |
| [`response`](executor/resolution.mdx#response) | 使用 zone record 模板构造完整 DNS 响应，包括记录区、RCODE 与响应标志。 | <PluginAvailability from="standard" compact /> |
| [`redirect`](executor/resolution.mdx#redirect) | 把请求域名改写为另一个目标域名，并在返回阶段补回客户端可见的 CNAME。 | <PluginAvailability from="minimal" compact /> |
| [`client_ip_from_ecs`](executor/resolution.mdx#client_ip_from_ecs) | 使用请求 ECS 地址覆盖当前请求的客户端 IP，供后续 matcher 与记录器使用。 | <PluginAvailability from="standard" compact /> |
| [`ecs_handler`](executor/resolution.mdx#ecs_handler) | 处理 EDNS Client Subnet：保留、改写或按来源 IP 自动补齐。 | <PluginAvailability from="minimal" compact /> |
| [`forward_edns0opt`](executor/resolution.mdx#forward_edns0opt) | 把指定 EDNS0 option code 从请求转发到最终响应中。 | <PluginAvailability from="minimal" compact /> |

</div>

### 响应改写

<div className="plugin-catalog">

| 插件 | 作用 | 内置版本 |
| --- | --- | --- |
| [`ttl`](executor/response.mdx#ttl) | 改写响应 TTL（统一值或上下限裁剪）。 | <PluginAvailability from="minimal" compact /> |
| [`ip_selector`](executor/response.mdx#ip_selector) | 对响应中的多个 IP 主动探测并按评分选择或重排地址。 | <PluginAvailability from="standard" compact /> |
| [`prefer_ipv4` / `prefer_ipv6`](executor/response.mdx#prefer_ipv4--prefer_ipv6) | 双栈优选器，对偏好类型做学习，对非偏好类型做抑制。 | <PluginAvailability from="minimal" compact /> |
| [`black_hole`](executor/response.mdx#black_hole) | 按 `nxdomain` / `nodata` / `null` / `custom` / `refused` 模式生成全 qtype 拦截响应。 | <PluginAvailability from="minimal" compact /> |
| [`drop_resp`](executor/response.mdx#drop_resp) | 清空当前上下文中的响应。 | <PluginAvailability from="minimal" compact /> |
| [`reverse_lookup`](executor/response.mdx#reverse_lookup) | 缓存应答中的 IP → 域名关系，并可选地处理 PTR 查询。 | <PluginAvailability from="standard" compact /> |

</div>

### 观测与调试

<div className="plugin-catalog">

| 插件 | 作用 | 内置版本 |
| --- | --- | --- |
| [`query_summary`](executor/observability.mdx#query_summary) | 在后续链路执行完后输出紧凑查询摘要。 | <PluginAvailability from="minimal" compact /> |
| [`query_recorder`](executor/observability.mdx#query_recorder) | 把请求、响应、`sequence` 路径事件持久化到 SQLite，并暴露历史查询、统计和 SSE 实时推送。 | <PluginAvailability from="standard" compact /> |
| [`metrics_collector`](executor/observability.mdx#metrics_collector) | 收集轻量级请求计数与延时指标，并导出 Prometheus 格式。 | <PluginAvailability from="standard" compact /> |
| [`debug_print`](executor/observability.mdx#debug_print) | 打印请求与响应对象，便于调试。 | <PluginAvailability from="minimal" compact /> |
| [`sleep`](executor/observability.mdx#sleep) | 异步延迟，用于测试和策略实验。 | <PluginAvailability from="minimal" compact /> |

</div>

### 副作用与系统联动

<div className="plugin-catalog">

| 插件 | 作用 | 内置版本 |
| --- | --- | --- |
| [`http_request`](executor/integrations.mdx#http_request) | 向外部 `http/https` 服务发送回调请求，适合 webhook、审计、告警和外部联动。 | <PluginAvailability from="standard" compact /> |
| [`learn_domain`](executor/observability.mdx#learn_domain) | 把流水线中的请求域名学习到 `dynamic_domain_set`，用于动态放行或拦截列表。 | <PluginAvailability from="standard" compact /> |
| [`script`](executor/integrations.mdx#script) | 执行外部命令，并把 `DnsContext` 中的稳定字段注入为参数或环境变量。 | <PluginAvailability from="standard" compact /> |
| [`ipset`](executor/integrations.mdx#ipset) | 把响应中的 IP 写入 Linux `ipset`（内置 netlink 后端，无需 `ipset` 命令）。 | <PluginAvailability from="full" compact /> |
| [`nftset`](executor/integrations.mdx#nftset) | 把响应 IP 写入 Linux `nftables set`（内置 netlink 后端，无需 `nft` 命令）。 | <PluginAvailability from="full" compact /> |
| [`ros_address_list`](executor/integrations.mdx#ros_address_list) | 把应答 IP 投影到 RouterOS `address-list`，由防火墙、mangle 或策略路由规则消费该集合。 | <PluginAvailability from="full" compact /> |
| [`ros_route`](executor/integrations.mdx#ros_route) | 把应答 IP 投影为 RouterOS routing table 中的逐 IP 静态路由，并应用配置的网关和 distance。 | <PluginAvailability from="full" compact /> |

</div>

### 维护与调度

<div className="plugin-catalog">

| 插件 | 作用 | 内置版本 |
| --- | --- | --- |
| [`upgrade`](executor/maintenance.mdx#upgrade) | 在执行器链路中触发 OxiDNS 升级流程。 | <PluginAvailability from="standard" compact /> |
| [`download`](executor/maintenance.mdx#download) | 下载一个或多个 `http/https` 文件到本地，并在写入完成后原子覆盖。 | <PluginAvailability from="standard" compact /> |
| [`reload_provider`](executor/maintenance.mdx#reload_provider) | 按 tag 定向重建 provider 内部快照，不触发整体配置 reload。 | <PluginAvailability from="minimal" compact /> |
| [`reload`](executor/maintenance.mdx#reload) | 触发与 `POST /reload` 相同的应用级全量 reload。 | <PluginAvailability from="minimal" compact /> |
| [`cron`](executor/maintenance.mdx#cron) | 后台调度一组 executor，按 cron 表达式或固定间隔触发任务。 | <PluginAvailability from="standard" compact /> |

</div>

## 匹配器插件（matcher）

详细字段见 [匹配器插件](matcher.mdx)。

### 请求维度

<div className="plugin-catalog">

| 插件 | 作用 | 内置版本 |
| --- | --- | --- |
| [`qname`](matcher/request.mdx#qname) | 匹配请求中的查询域名。 | <PluginAvailability from="minimal" compact /> |
| [`question`](matcher/request.mdx#question) | 按 provider 的 `contains_question` 语义匹配请求中的 question。 | <PluginAvailability from="minimal" compact /> |
| [`qtype`](matcher/request.mdx#qtype) | 匹配请求中的 qtype。 | <PluginAvailability from="minimal" compact /> |
| [`qclass`](matcher/request.mdx#qclass) | 匹配请求中的 qclass。 | <PluginAvailability from="minimal" compact /> |
| [`client_ip`](matcher/request.mdx#client_ip) | 匹配客户端来源 IP。 | <PluginAvailability from="minimal" compact /> |
| [`ptr_ip`](matcher/request.mdx#ptr_ip) | 从 PTR 请求名中解析 IP 并做匹配。 | <PluginAvailability from="minimal" compact /> |

</div>

### 响应维度

<div className="plugin-catalog">

| 插件 | 作用 | 内置版本 |
| --- | --- | --- |
| [`resp_ip`](matcher/response.mdx#resp_ip) | 匹配响应 answers 中的 A / AAAA IP。 | <PluginAvailability from="minimal" compact /> |
| [`cname`](matcher/response.mdx#cname) | 匹配响应中的 CNAME 目标域名。 | <PluginAvailability from="minimal" compact /> |
| [`rcode`](matcher/response.mdx#rcode) | 匹配当前响应的 rcode。 | <PluginAvailability from="minimal" compact /> |
| [`has_resp`](matcher/response.mdx#has_resp) | 只要上下文中已有响应就命中。 | <PluginAvailability from="minimal" compact /> |
| [`has_cname`](matcher/response.mdx#has_cname) | 判断响应 Answer 区是否包含 CNAME 记录。 | <PluginAvailability from="minimal" compact /> |
| [`has_wanted_ans`](matcher/response.mdx#has_wanted_ans) | 判断响应 answers 中是否包含与请求 qtype 对应的记录。 | <PluginAvailability from="minimal" compact /> |

</div>

### 上下文与表达式

<div className="plugin-catalog">

| 插件 | 作用 | 内置版本 |
| --- | --- | --- |
| [`mark`](matcher/context.mdx#mark) | 匹配上下文中的 mark 集合。 | <PluginAvailability from="minimal" compact /> |
| [`env`](matcher/context.mdx#env) | 匹配进程环境变量。 | <PluginAvailability from="minimal" compact /> |
| [`time`](matcher/context.mdx#time) | 按时区、时间段、星期和每月日期匹配。 | <PluginAvailability from="minimal" compact /> |
| [`random`](matcher/context.mdx#random) | 按概率命中，适合灰度和采样。 | <PluginAvailability from="minimal" compact /> |
| [`rate_limiter`](matcher/context.mdx#rate_limiter) | 基于客户端 IP 的令牌桶限流。 | <PluginAvailability from="minimal" compact /> |
| [`string_exp`](matcher/composition.mdx#string_exp) | 通用字符串表达式匹配器，补足专用 matcher 不够灵活的场景。 | <PluginAvailability from="minimal" compact /> |

</div>

### 组合与常量

<div className="plugin-catalog">

| 插件 | 作用 | 内置版本 |
| --- | --- | --- |
| [`any_match`](matcher/composition.mdx#any_match) | 组合多个 matcher 表达式，任意一个命中即返回 `true`。 | <PluginAvailability from="minimal" compact /> |
| [`_true`](matcher/composition.mdx#_true) | 恒为真。 | <PluginAvailability from="minimal" compact /> |
| [`_false`](matcher/composition.mdx#_false) | 恒为假。 | <PluginAvailability from="minimal" compact /> |

</div>

## 数据提供器插件（provider）

详细字段见 [数据提供器插件](provider.mdx)。

<div className="plugin-catalog">

| 插件 | 作用 | 内置版本 |
| --- | --- | --- |
| [`domain_set`](provider/domain.mdx#domain_set) | 高性能域名规则集合，可被 `qname`、`cname` 等插件引用。 | <PluginAvailability from="minimal" compact /> |
| [`dynamic_domain_set`](provider/domain.mdx#dynamic_domain_set) | 可写的本地域名规则文件，支持热快照匹配、API 管理和自动学习写入。 | <PluginAvailability from="standard" compact /> |
| [`geosite`](provider/domain.mdx#geosite) | 从 v2ray-rules-dat 的 `geosite.dat` 中提取一个或多个 code，并编译成可复用域名规则集合。 | <PluginAvailability from="standard" compact /> |
| [`adguard_rule`](provider/domain.mdx#adguard_rule) | 提供 AdGuard Home DNS 规则子集的可复用 provider。 | <PluginAvailability from="standard" compact /> |
| [`ip_set`](provider/ip.mdx#ip_set) | IP / CIDR 规则集合，可被 `client_ip`、`resp_ip`、`ptr_ip` 等 matcher 引用。 | <PluginAvailability from="minimal" compact /> |
| [`geoip`](provider/ip.mdx#geoip) | 从 v2ray-rules-dat 的 `geoip.dat` 中提取一个或多个 code，并编译成可复用 IP / CIDR 集合。 | <PluginAvailability from="standard" compact /> |

</div>
