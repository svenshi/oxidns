![OxiDNS Banner](.github/img/logo-banner.png)

[![oxidns downloads](https://img.shields.io/github/downloads/SvenShi/oxidns/total)](https://github.com/SvenShi/oxidns/releases)
[![latest release](https://img.shields.io/github/v/release/svenshi/oxidns)](https://github.com/svenshi/oxidns/releases/latest)
[![license](https://img.shields.io/github/license/svenshi/oxidns)](LICENSE)
[![Rust CI](https://github.com/svenshi/oxidns/actions/workflows/rust-ci.yml/badge.svg?branch=main)](https://github.com/svenshi/oxidns/actions/workflows/rust-ci.yml)
[![WebUI CI](https://github.com/svenshi/oxidns/actions/workflows/webui-ci.yml/badge.svg)](https://github.com/svenshi/oxidns/actions/workflows/webui-ci.yml)

[中文](README.md) | [English](README_EN.md) · [文档](https://oxidns.org/) · [快速开始](https://oxidns.org/quickstart) · [插件参考](https://oxidns.org/plugin-reference/overview)

# OxiDNS

**面向复杂网络的高性能 DNS 策略编排引擎。**

OxiDNS 是一个使用 Rust 构建、面向软路由、OpenWrt、Homelab 和高级自建网络的插件化 DNS 引擎。它通过声明式策略组合匹配、缓存、转发、回退、改写、本地应答和系统联动，并提供 WebUI、管理 API、查询记录、Prometheus 指标和实时日志。

项目受 [mosdns](https://github.com/IrineSistiana/mosdns) 启发，但不止于规则分流：OxiDNS 将 DNS 请求、上游响应和网络副作用放进同一条可组合、可解释的策略管线，让复杂行为仍然能够被配置、验证和追踪。

[快速开始](https://oxidns.org/quickstart) · [配置总览](https://oxidns.org/configuration) · [常见场景](https://oxidns.org/scenarios) · [性能与基准](https://oxidns.org/benchmarks)

## 为什么选择 OxiDNS

- **策略可编排**：`sequence` 将 matcher、executor 和 provider 组合成可复用的条件分支、跳转和回退链。
- **决策可解释**：查询记录、执行路径、结构化日志和指标可以回答一次查询匹配了什么、执行了什么、为何选择当前结果。
- **接入与出口统一**：同一策略可服务 UDP、TCP、DoT、DoQ 和 DoH，并统一管理上游解析、连接复用、并发裁决与代理出口。
- **DNS 驱动网络行为**：解析结果可以直接联动 Linux `ipset` / `nftset`、RouterOS 地址列表与静态路由、HTTP webhook 和外部脚本。

OxiDNS 使用自有 DNS 消息模型与 wire 编解码层，并把规则编译、依赖分析和连接初始化尽可能移出请求热路径；设计方法和历史测试结果见[性能与基准](https://oxidns.org/benchmarks)。

核心请求路径保持清晰：

```text
client -> server -> sequence (matcher + executor + provider)
                         |-> upstream / local answer -> response
                         `-> side effects
```

## 策略配置是什么样的

下面的配置把 `corp.lan` 交给内网 DNS，其余域名使用加密上游。Provider、matcher、executor 和 server 通过 tag 组合，不需要把策略写进协议入口：

```yaml
plugins:
  - tag: internal_domains
    type: domain_set
    args:
      exps: ["domain:corp.lan"]

  - tag: forward_internal
    type: forward
    args:
      upstreams:
        - addr: "udp://192.168.1.1:53"

  - tag: forward_public
    type: forward
    args:
      upstreams:
        - addr: "tls://dns.quad9.net:853"
          bootstrap: "9.9.9.9:53"

  - tag: main
    type: sequence
    args:
      - matches: "qname $internal_domains"
        exec: "$forward_internal"
      - matches: "!has_resp"
        exec: "$forward_public"
      - exec: accept

  - tag: dns_server
    type: udp_server
    args:
      entry: main
      listen: ":5335"
```

完整配置结构、复用规则和更多部署模板见[配置总览](https://oxidns.org/configuration)与[常见场景](https://oxidns.org/scenarios)。

---

## 核心能力

| 类别 | 能力 |
| --- | --- |
| 协议接入 | UDP、TCP、DoT、DoQ、DoH（HTTP/1.1、HTTP/2、HTTP/3） |
| 策略编排 | `sequence`、条件匹配、执行器组合、跳转与回退链 |
| 上游与出口 | 多协议上游、并发响应裁决、连接复用、bootstrap、SOCKS5、统一 `network.outbound` |
| 响应处理 | TTL 感知缓存与负缓存、ECS 与 ECS 客户端地址映射、本地记录、重定向、响应构造、双栈与 IP 优选 |
| 规则数据 | 域名与 IP 集、GeoIP、GeoSite、AdGuard 规则、动态域名学习 |
| 系统联动 | Linux `ipset` / `nftset`、RouterOS address-list / static route、HTTP webhook、外部脚本 |
| 可观测性 | 查询审计与执行路径、实时日志、Prometheus 指标、上游探测、健康检查 |
| 运行时管理 | 配置校验与热重载、matcher 基础结果固定、provider 定向重载、缓存与升级管理 |
| 部署能力 | 多平台构建、Debian 包、OpenWrt LuCI 插件、内置 WebUI 与管理 API、服务化安装 |

完整的内置组件和配置字段见[插件参考](https://oxidns.org/plugin-reference/overview)。

内置标准模式可把上游组、独立解析路径、智能分流、专属解析组、有界动态学习、高级规则与场景模板编译为纯 OxiDNS 原生插件图，并提供编译解释、有界单查询诊断、语义 Diff、意图资产和安全回滚；它不控制 OpenWrt、系统 DNS、DHCP、防火墙或第三方代理。详见[标准模式运维解释](https://oxidns.org/standard-mode-operations)。

---

## 快速开始

一条命令安装最新 release，并默认注册和启动为系统服务：

```bash
curl -fsSL https://oxidns.org/install.sh | sudo sh
```

Windows 管理员 PowerShell：

```powershell
irm https://oxidns.org/install.ps1 | iex
```

默认情况下，安装脚本会下载对应平台的 release、安装 WebUI，并注册和启动系统服务。

安装完成后，可以验证管理端和默认 DNS 服务：

```bash
curl -fsS http://127.0.0.1:9199/api/health
dig @127.0.0.1 example.com
dig @127.0.0.1 example.com +tcp
```

WebUI 默认地址为 `http://服务器IP:9199/`。允许其他设备访问前，请启用 `api.http.auth` 并通过防火墙或反向代理限制来源。修改配置后，请先使用 `oxidns check` 校验。

OpenWrt 会通过同一个安装脚本安装 [`luci-app-oxidns`](https://github.com/svenshi/luci-app-oxidns)。完成首次查询见[快速开始](https://oxidns.org/quickstart)；手动下载、Docker、Debian 包、便携安装和卸载见文档中的“安装与部署”；按需裁剪协议和插件见[自定义编译](https://oxidns.org/custom-build)。

## 适合谁

OxiDNS 适合需要长期运行、精细控制和完整可观测性的 DNS 环境，例如：

- 家庭网关、旁路由、OpenWrt、NAS 和 Homelab
- 按域名、客户端、查询类型或响应结果进行策略路由
- 多上游并发、主备回退、加密 DNS 与自定义出口
- 广告过滤、本地覆盖、双栈偏好、ECS 和动态规则学习
- 由 DNS 结果驱动防火墙集合、地址列表或策略路由

它不是权威 DNS 服务，也不是无需理解配置模型的一键广告过滤面板。如果你的首要需求是开箱即用的图形化管理，其他面向该场景的产品可能更合适；OxiDNS 更适合愿意用一定配置复杂度换取控制力和可解释性的用户。

---

## 文档

- **开始使用**：[快速开始](https://oxidns.org/quickstart) · [配置总览](https://oxidns.org/configuration) · [常见场景](https://oxidns.org/scenarios) · [从 mosdns 迁移](https://oxidns.org/migrate-from-mosdns)
- **参考手册**：[插件总览](https://oxidns.org/plugin-reference/overview) · [管理 API](https://oxidns.org/api) · [命令行工具](https://oxidns.org/cli) · [DNS 编码速查](https://oxidns.org/dns-codes)
- **部署运维**：[运维与故障排查](https://oxidns.org/operations) · [安全加固](https://oxidns.org/security) · [WebUI](https://oxidns.org/webui) · [OpenWrt](https://oxidns.org/openwrt)
- **了解项目**：[架构与设计](https://oxidns.org/architecture-and-design) · [性能与基准](https://oxidns.org/benchmarks) · [文档版本](https://oxidns.org/documentation) · [版本更新](https://oxidns.org/releases) · [路线图](https://oxidns.org/roadmap)
- **参与社区**：[贡献指南](https://oxidns.org/contributing) · [支持项目开发](https://oxidns.org/support-development) · [GitHub Discussions](https://github.com/svenshi/oxidns/discussions) · [Telegram](https://t.me/oxidns)

## 项目状态

OxiDNS 仍在持续开发，适合高级家庭网络、软路由、Homelab 和其他可控的自建网络环境。用于生产或关键网络前，请完成配置与回退验证、容量评估和监控建设。DNS 配置会直接影响网络可用性，请在变更前保留可恢复的配置和旁路方案。

欢迎提交 Issue、反馈真实场景、改进文档或贡献插件。

---

## 社区交流

欢迎加入 Telegram 群与作者和其他用户交流：[**@OXIDNS** · https://t.me/oxidns](https://t.me/oxidns)

<a href="https://t.me/oxidns">
  <img src=".github/img/telegram-qr.png" alt="OxiDNS Telegram 群二维码" width="220" />
</a>

---

## 支持项目开发

如果 OxiDNS 对你有帮助，可以通过微信或支付宝支持项目的持续开发与维护。支持完全自愿，感谢每一份认可。

<table>
  <tr>
    <th align="center">微信支付</th>
    <th align="center">支付宝</th>
  </tr>
  <tr>
    <td align="center"><img src="docs/static/img/support/wechat-pay.jpg" alt="支持 OxiDNS 项目开发的微信支付收款码" width="260" /></td>
    <td align="center"><img src="docs/static/img/support/alipay.jpg" alt="支持 OxiDNS 项目开发的支付宝收款码" width="260" /></td>
  </tr>
</table>

资金支持不会建立商业服务关系，也不会影响功能规划或问题处理的优先级。其他支持方式请参阅[贡献指南](https://oxidns.org/contributing)。

---

## 许可证

本项目基于 [GNU General Public License v3.0 or later](LICENSE) 开源。
