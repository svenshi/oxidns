---
title: 运行、探测与系统服务
---


本页说明前台运行、上游探测和操作系统服务管理命令。所有相对运行路径都应使用与生产服务一致的工作目录。

## `start`

前台启动 OxiDNS 服务。

典型用法：

```bash
oxidns start -c config.yaml
oxidns start -c config.yaml -l debug
oxidns start -c /etc/oxidns/config.yaml -d /var/lib/oxidns
```

参数说明：

- `-c, --config <PATH>`
  - 配置文件路径。
  - 默认值：`config.yaml`
- `-d, --working-dir <PATH>`
  - 启动前切换到指定工作目录。
  - 所有运行期相对路径都以该目录为基准，包括日志、SQLite、规则文件和 `api.http.webui.root`。
  - Debian 默认布局中，配置放在 `/etc/oxidns/config.yaml`，运行期相对路径资源放在 `/var/lib/oxidns`。
- `-l, --log-level <LEVEL>`
  - 临时覆盖配置文件中的日志级别。
  - 支持：`off` `trace` `debug` `info` `warn` `error`

适用场景：

- 本地调试
- 前台运行
- 容器内直接启动

## `probe`

主动探测运行时外部目标。当前提供 `probe upstream`，用于检查单个 DNS 上游的连通性、基础响应信息、域名解析结果，以及并发 / pipeline 行为。

### `probe upstream`

典型用法：

```bash
oxidns probe upstream udp://1.1.1.1:53
oxidns probe upstream tcp://1.1.1.1:53
oxidns probe upstream tls://dns.google:853 --qname example.com. --qtype A
oxidns probe upstream https://dns.google/dns-query --json
oxidns probe upstream tcp://dns.example.com:53 -c config.yaml --outbound remote
```

参数说明：

- `<addr>`
  - 要探测的 upstream 地址。
  - 支持与 forward upstream 一致的地址写法，例如 `udp://`、`tcp://`、`tcp+pipeline://`、`tls://`、`tls+pipeline://`、`https://`、`doh://`、`h3://`、`quic://`、`doq://`。
  - 不带 scheme 时按 UDP 处理。
- `-c, --config <PATH>`
  - 可选读取配置文件，只复用其中的 `network.outbound` profile。
  - 未指定时不会读取运行配置。
- `-d, --working-dir <PATH>`
  - 读取配置前切换工作目录。
- `--outbound <NAME>`
  - 使用指定 outbound profile 的 resolver / proxy 设置。
- `--dial-addr <IP>`
  - 直接连接指定 IP，同时保留 `<addr>` 中的主机名用于 TLS SNI 和 HTTP Host。
- `--bootstrap <ADDR>`
  - 使用指定 bootstrap DNS 解析域名型 upstream。
- `--bootstrap-version <4|6>`
  - bootstrap 解析时偏好的 IP 版本。
- `--socks5 <ADDR>`
  - 对支持代理的上游连接使用 SOCKS5。
- `--port <PORT>`
  - 覆盖 upstream 端口。
- `--insecure-skip-verify`
  - 跳过 TLS 证书校验，仅建议测试时使用。
- `--timeout <DURATION>`
  - 单次查询超时。
  - 默认值：`5s`
- `--qname <NAME>`
  - 串行基线查询域名。
  - 默认值：`example.com.`
- `--qtype <TYPE>`
  - 查询类型。
  - 默认值：`A`
- `--serial-samples <N>`
  - 串行基线查询次数。
  - 默认值：`2`
- `--pipeline-concurrency <N>`
  - 并发探测的查询数量；TCP / DoT 会强制在同一条连接上发送这些查询。
  - 默认值：`16`
- `--pipeline-rounds <N>`
  - 并发探测轮数。
  - 默认值：`2`
- `--json`
  - 输出结构化 JSON 报告。

输出内容：

- 目标信息：地址、协议、服务名、端口、超时。
- 域名型 upstream 的解析结果：`resolved_ip` 和 `resolution_source`，来源可能是 `literal`、`dial_addr`、`configured`、`bootstrap`、`system` 或 `proxy`。
- 串行基线：reachable / unreachable、平均延迟、rcode、answer 数量、TC / RA 标志和错误摘要。
- 并发探测：supported / unsupported / unstable / inconclusive、成功数、超时数、响应 ID / question / qtype 不匹配数、其它错误和建议。
- 非 JSON 模式会在探测过程中向 stderr 输出进度，最终报告输出到 stdout；JSON 模式只向 stdout 输出报告。

协议行为：

- UDP、DoH、DoH3 和 DoQ 使用对应 upstream 实现发起并发查询，用来评估该协议下的并发或多路复用表现。
- TCP 和 DoT 会额外强制使用同一条连接发送并发查询，用来发现开启 pipeline 后可能出现的超时、连接关闭、协议错误、响应 ID 错乱或 question 串线。
- 如果串行基线失败，并发结论会是 `inconclusive`，避免把基础连通性问题误判成 pipeline 问题。

## `service`

管理系统服务安装与运行状态。

支持以下子命令：

- `service install`
- `service start`
- `service stop`
- `service restart`
- `service uninstall`

### `service install`

安装系统服务定义，但不会立即启动。

```bash
sudo oxidns service install -d /var/lib/oxidns -c /etc/oxidns/config.yaml
```

参数说明：

- `-d, --working-dir <PATH>`
  - 服务工作目录，也是服务内所有运行期相对路径的基准。
  - 必须为绝对路径。
  - 生成的服务会通过 `ExecStart ... -d <PATH>` 传给 OxiDNS；自定义 systemd unit 若额外设置 `WorkingDirectory=`，请保持二者一致。
- `-c, --config <PATH>`
  - 服务启动时使用的配置文件路径。

### `service start`

启动已安装的系统服务。

```bash
sudo oxidns service start
```

### `service stop`

停止已安装的系统服务。

```bash
sudo oxidns service stop
```

### `service restart`

重启已安装的系统服务。

```bash
sudo oxidns service restart
```

### `service uninstall`

卸载已安装的系统服务。

```bash
sudo oxidns service uninstall
```
