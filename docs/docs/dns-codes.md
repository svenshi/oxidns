---
title: DNS 编码速查表
sidebar_position: 4
---

OxiDNS 在 `qtype`、`qclass`、`rcode` matcher 以及 `reject [rcode]` 等配置中，通常同时支持十进制数字和英文助记名。英文助记名大小写不敏感，例如 `SERVFAIL`、`servfail`、`ServFail` 都可以表示同一个 RCODE。

未知或未来扩展的 `QTYPE`、`QCLASS`、`RCODE` 可以在 matcher 中使用十进制数字匹配。`reject [rcode]` 是例外：它只能生成基础 DNS RCODE `0..15`，不会自动生成需要 EDNS OPT 表示的扩展 RCODE。

## RCODE 响应码

| 数字 | Code 表示 | 意义 |
| --- | --- | --- |
| `0` | `NOERROR` | 查询成功，响应没有错误。没有答案时通常表示 NODATA，而不是域名不存在。 |
| `1` | `FORMERR` | 请求格式错误，服务器无法解析请求。 |
| `2` | `SERVFAIL` | 服务器处理失败，常见于上游故障、验证失败或内部错误。 |
| `3` | `NXDOMAIN` | 域名不存在。 |
| `4` | `NOTIMP` | 服务器不支持请求的操作。 |
| `5` | `REFUSED` | 服务器拒绝处理请求，常用于策略拒绝或权限限制。 |
| `6` | `YXDOMAIN` | DNS UPDATE 中使用：本不应存在的名称已经存在。 |
| `7` | `YXRRSET` | DNS UPDATE 中使用：本不应存在的 RRSet 已经存在。 |
| `8` | `NXRRSET` | DNS UPDATE 中使用：应存在的 RRSet 不存在。 |
| `9` | `NOTAUTH` | 服务器对该区域不具权威性，或操作未授权。 |
| `10` | `NOTZONE` | 名称不属于指定区域。 |
| `11..15` | 未分配 | 基础 RCODE 保留空段。`reject` 可接受这些数字，但通常不建议用于常规策略。 |
| `16` | `BADVERS` / `BADSIG` | EDNS 中表示 OPT 版本错误；TSIG 中表示签名失败，具体含义取决于上下文。 |
| `17` | `BADKEY` | TSIG/TKEY key 无法识别。 |
| `18` | `BADTIME` | TSIG 签名时间窗口不合法。 |
| `19` | `BADMODE` | TKEY 模式错误。 |
| `20` | `BADNAME` | TKEY 名称重复或不合法。 |
| `21` | `BADALG` | 算法不支持。 |
| `22` | `BADTRUNC` | TSIG 截断错误。 |
| `23` | `BADCOOKIE` | EDNS Cookie 缺失或错误。 |

常见策略写法：

```yaml
- matches: "rcode SERVFAIL,3"
  exec: "forward 8.8.8.8"

- matches: "qname domain:blocked.example"
  exec: "reject NXDOMAIN"
```

## QCLASS 查询类别

| 数字 | Code 表示 | 意义 |
| --- | --- | --- |
| `1` | `IN` | Internet，绝大多数普通 DNS 查询都使用这个类别。 |
| `2` | `CS` | CSNET，历史类别，现已很少使用。 |
| `3` | `CH` | CHAOS，常见于少量诊断查询。 |
| `4` | `HS` | Hesiod，历史类别。 |
| `254` | `NONE` | DNS UPDATE 中使用，表示没有类别。 |
| `255` | `ANY` / `*` | 任意类别，主要用于特殊查询或匹配表达。 |

常见策略写法：

```yaml
- matches: "qclass IN"
  exec: "$forward_main"
```

## QTYPE 查询类型

| 数字 | Code 表示 | 意义 |
| --- | --- | --- |
| `1` | `A` | IPv4 地址记录。 |
| `2` | `NS` | 权威名称服务器记录。 |
| `5` | `CNAME` | 规范名称别名记录。 |
| `6` | `SOA` | 区域起始授权记录，也常用于负缓存 TTL 判断。 |
| `12` | `PTR` | 反向解析指针记录。 |
| `15` | `MX` | 邮件交换记录。 |
| `16` | `TXT` | 文本记录，常用于 SPF、验证和策略文本。 |
| `28` | `AAAA` | IPv6 地址记录。 |
| `33` | `SRV` | 服务定位记录。 |
| `35` | `NAPTR` | 命名授权指针记录，常用于服务发现。 |
| `41` | `OPT` | EDNS 伪记录，不是普通资源记录。 |
| `43` | `DS` | DNSSEC 委托签名记录。 |
| `46` | `RRSIG` | DNSSEC 记录签名。 |
| `47` | `NSEC` | DNSSEC 否定证明记录。 |
| `48` | `DNSKEY` | DNSSEC 公钥记录。 |
| `50` | `NSEC3` | DNSSEC NSEC3 否定证明记录。 |
| `51` | `NSEC3PARAM` | NSEC3 参数记录。 |
| `52` | `TLSA` | DANE TLSA 证书关联记录。 |
| `64` | `SVCB` | 服务绑定记录。 |
| `65` | `HTTPS` | HTTPS 服务绑定记录。 |
| `99` | `SPF` | SPF 专用记录类型；实际部署中更常见的是 `TXT`。 |
| `249` | `TKEY` | 动态密钥协商记录。 |
| `250` | `TSIG` | 事务签名伪记录。 |
| `251` | `IXFR` | 增量区域传送。 |
| `252` | `AXFR` | 完整区域传送。 |
| `255` | `ANY` / `*` | 请求任意类型记录；实际递归解析器通常会限制或最小化响应。 |
| `256` | `URI` | URI 记录。 |
| `257` | `CAA` | 证书颁发机构授权记录。 |
| `261` | `RESINFO` | 解析器信息记录。 |

常见策略写法：

```yaml
- matches: "qtype A,AAAA,HTTPS,65"
  exec: "$forward_main"
```

如果需要匹配表中没有列出的类型，可以直接写十进制数字；如果 OxiDNS 已内置该类型名称，也可以写对应英文助记名。
