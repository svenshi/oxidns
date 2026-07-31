---
title: 标准模式高级策略
---

标准模式 schema v6 在同一后端 PolicyPlan 中提供专属解析组、有界动态学习、高级规则和完整场景模板。所有能力只生成 OxiDNS 原生 provider、matcher、executor 与 UDP/TCP listener，不读取或修改 OpenWrt/UCI、系统 DNS、DHCP、防火墙、`ipset`、`nftset`、RouterOS 或第三方控制器。

## 专属解析组

一个专属组聚合域名规则、内嵌上游组、过滤/日志/缓存/ECS/双栈/IP 优选策略以及可选的原生 UDP/TCP listener。删除聚合对象后，下一次生成不会残留 provider、matcher、forward、path、cache 或 listener tag。额外 listener 只接收显式发送到该端口的请求，不声称接管主机 DNS。

## 动态学习

每个 profile 通过 QTYPE、RCODE、wanted answer 和可选响应 IP 角色分类成功响应，再写入独立 `dynamic_domain_set`。学习路由低于手工 allow/block、设备、专属组和手工强制路由。默认 `continue` 使用有界异步队列，写入失败不改变 DNS 响应；只有显式 `fail_closed` 会把错误传回执行链。

规则文件与 metadata 侧车文件由 profile ID 派生。最大条目数采用整批 reject-new；TTL 只淘汰 learned 来源，API 手工修正为 manual 来源。WebUI 依据生成 tag 提供状态、分页、添加/删除/清空以及学习暂停/恢复。

## 高级规则

请求阶段支持域名、客户端、QTYPE、IANA 时区时间段和 rate-limit exceeded 的 AND 组合，可选择路径或生成阻断响应。响应阶段要求精确一个 source path，可组合 CNAME、RCODE、wanted answer、QTYPE 和响应 IP provider，然后重路由到隔离目标 path。目标变体不会再次进入来源规则，因此没有递归环。

响应重路由 `fail_open` 会在目标失败时保留原始响应；`fail_closed` 返回明确的 SERVFAIL 或 REFUSED。多上游共识仍使用原生 `forward.response_selection: consensus`，至少需要两个启用上游。

## 场景模板

`low_latency`、`privacy_dns`、`internal_domains` 与 `regional_upstream` 由后端确定性展开。预览返回完整拟议 intent、对象差异、诊断、生成 YAML、tag map 和预检结果；命名空间碰撞不会覆盖旧对象。操作者接受预览后只进入 WebUI 草稿，最终变更仍走正常 Plan/Apply 事务。
