---
title: 规则与 provider 语法
---

本页汇总多个 matcher、executor 和 provider 共用的域名、IP 与数据源引用语法。


## 通用规则语法

### 域名规则

以下规则会出现在 `qname`、`cname`、`domain_set`、`hosts`、`redirect` 等插件中：

- `full:example.com`
  - 完整匹配。
- `domain:example.com`
  - 后缀匹配。
- `keyword:cdn`
  - 子串匹配。
- `regexp:^api[0-9]+\\.example\\.com$`
  - 正则匹配。
- `example.com`
  - 未写前缀时，`qname`、`cname`、`domain_set` 等通用域名规则通常等价于 `domain:example.com`；`hosts` 和 `redirect` 按 `full:example.com` 精确匹配处理。

`hosts` 的上述域名规则仅用于默认的 `format: oxidns`。设置 `format: standard` 后，规则为 `<IP> <主机名> [别名…]`，主机名均精确匹配，不解释匹配前缀；同名主机的地址合并去重。

### IP 规则

以下规则会出现在 `client_ip`、`resp_ip`、`ptr_ip`、`ip_set` 等插件中：

- 单个 IP：`1.1.1.1`
- 网段：`192.168.0.0/16`
- IPv6 网段：`2400:3200::/32`

### provider 引用

支持在 matcher 或 provider 参数中引用 provider：

- `$tag`
  - 引用已定义且具备对应匹配能力的 provider。
  - 例如域名场景可引用 `domain_set`、`geosite`。
  - 例如 IP 场景可引用 `ip_set`、`geoip`。
- `&/path/to/file`
  - 直接从文件加载规则。

示例：

```yaml
args:
  - "domain:example.com"
  - "$core_domains"
  - "&/etc/oxidns/domains.txt"
```
