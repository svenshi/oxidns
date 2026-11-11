---
title: 全局配置
---

本页说明所有插件共享的 YAML 装载规则和 OxiDNS 顶层配置。插件专属字段请查阅[插件参考](../plugin-reference/overview.md)。

## 写在最前

OxiDNS 的配置文件是 YAML。日常修改配置时，可以先把它理解为六个顶层部分：

```yaml
runtime:
  worker_threads: 4

api:
  http: "127.0.0.1:9088"

log:
  level: info
  file: ./oxidns.log

network:
  outbound:
    default: direct
    profiles:
      direct:
        resolver: system
        proxy: none

include: []

plugins:
  - tag: seq_main
    type: sequence
    args:
      - exec: "forward 1.1.1.1"
```

其中：

- `runtime`
  - 运行时参数。
- `api`
  - 管理 API。
- `log`
  - 日志输出。
- `network`
  - 共享网络出站配置，例如 HTTP 下载、升级检查和 webhook 请求使用的解析器与代理。
- `include`
  - 从其他配置文件载入插件定义。
- `plugins`
  - 所有插件实例定义。OxiDNS 通过插件组合完成完整 DNS 流程。

修改完成后，建议先校验再启动：

```bash
oxidns check -c config.yaml
```

如果配置中使用了相对路径，并且实际工作目录不是配置文件所在目录，可以配合 `-d` 指定工作目录。`-d` 是日志、SQLite、规则文件、`api.http.webui.root` 等所有运行期相对路径的统一基准，不会因为配置文件位于 `/etc/oxidns` 而自动改到配置目录：

```bash
oxidns check -c /etc/oxidns/config.yaml -d /var/lib/oxidns
```

Debian 默认布局中，配置文件放在 `/etc/oxidns/config.yaml`，运行期相对路径资源放在 `/var/lib/oxidns`。

尚未确定插件组合方式时，建议先阅读《[常见策略场景](../scenarios.md)》，再回到本页查询字段含义。

## 插件 tag 规则

`plugins[].tag` 是插件实例的全局唯一机器标识，同时直接用于管理 API 路径：`/api/plugins/{tag}/...`。因此 tag 必须满足以下规则：

- 长度为 1 到 64 个 ASCII 字符；
- 仅可使用英文字母、数字、`_`、`-` 和 `.`；
- `.` 只能分隔非空名称段，且每一段必须以字母或数字开头和结尾；
- `qs.exec.`、`qs.match.`、`qs.cron.` 为 Quick Setup 保留前缀，不能在用户配置中使用。

例如 `cache_main`、`cache.cn`、`prod.cache-01` 合法；`.`、`..`、`cache..cn`、`_cache`、`cache-`、`国内缓存` 和 `cache/main` 不合法。

从旧版本升级时，请先使用新二进制执行 `oxidns check -c config.yaml`。如需修改历史 tag，必须同步更新所有 `$tag`、`jump/goto`、插件依赖和其他对该 tag 的引用；OxiDNS 不会自动重命名，以免静默改变请求处理逻辑。

## 环境变量替换

OxiDNS 在启动、`oxidns check`、管理 API 配置校验和保存前校验时，先把 YAML **解析成数据结构**，再在字符串标量内部展开 `${VAR}` 占位符。`config.yaml` 文件本身不会被改写；WebUI 读取和保存配置时看到的仍然是原始占位符。

支持的写法：

| 写法 | 行为 |
| --- | --- |
| `${VAR}` | 使用进程环境变量 `VAR` 的值；未定义时报错 |
| `${VAR:-default}` | `VAR` 未定义或为空字符串时使用 `default` |
| `${env:VAR}` | 显式读取进程环境变量 `VAR`，可用于变量名与运行期占位符冲突的场景 |
| `${env:VAR:-default}` | 显式读取进程环境变量 `VAR`，未定义或为空字符串时使用 `default` |
| `$${...}` | 输出字面量 `${...}` |

`script`、`http_request` 等执行器使用的运行期占位符会被保留到请求执行阶段再渲染，例如 `${qname}`、`${client_ip}`、`${resp_ip}` 不会在配置加载时当作进程环境变量处理。如果确实需要读取同名环境变量，请使用显式写法，例如 `${env:qname}`。

未定义变量会立即报错，错误中包含变量名和发生位置的 YAML 路径（例如 `plugins[0].args.password`），避免空密码、空证书路径等问题静默通过。

示例：

```yaml
api:
  http:
    listen: ${API_LISTEN:-0.0.0.0:8080}
    ssl:
      cert: ${API_TLS_CERT}
      key: ${API_TLS_KEY}
    auth:
      type: basic
      username: ${ADMIN_USER}
      password: ${ADMIN_PASS}
```

因为替换发生在 YAML 解析之后，环境变量值可以包含任意字符——`*`、`&`、`:`、`#`、`'`、`"`、`\`、换行甚至二进制字节——都不会破坏配置文件的语法。不需要为含特殊字符的值手动加引号。当整段标量恰好等于一个占位符时（例如 `timeout: ${CACHE_TTL}`），展开结果会按 YAML 1.2 标量规则做一次类型恢复，所以数字、布尔、`null` 形态的环境变量仍能匹配数字 / 布尔 / 空类型字段；其他位置一律按字符串处理。`include` 路径同样支持占位符，例如：

```yaml
include:
  - ${OXIDNS_CONF_DIR}/plugins/common.yaml
```

## 顶层字段

### `include`

```yaml
# []string, 从其他配置文件载入 plugins 插件设置。
include:
  - ./plugins/common.yaml
  - ./plugins/server.yaml
```

字段说明：

- `include`
  - 只载入被包含文件中的 `plugins`，不会合并被包含文件的 `runtime`、`api` 或 `log`。
  - 插件合并顺序为：先按数组顺序递归载入 `include`，再追加当前文件的 `plugins`。
  - 相对路径以声明该 `include` 的配置文件所在目录为基准。
  - 最多递归 8 层。
  - 合并后的所有插件 `tag` 仍必须全局唯一。

### `runtime`

```yaml
runtime:
  worker_threads: 4
```

字段说明：

- `worker_threads`
  - 含义：Tokio 多线程运行时的 worker 数。
  - 默认：未配置时自动取系统可用并行度。
  - 限制：不能为 `0`。

### `log`

日志时间使用启动时识别的系统本地时区，标准输出、文件和 API 日志缓冲区保持一致。OpenWrt 未提供 `/etc/localtime` 时，会读取 `/etc/TZ` 中的 POSIX 时区规则；显式 `TZ` 环境变量优先。修改系统时区后，重启 OxiDNS 以更新日志时区。

```yaml
log:
  level: info
  file: ./oxidns.log
  rotation:
    type: daily
    max_files: 7
```

字段说明：

- `level`
  - 可选值：`off` `trace` `debug` `info` `warn` `error`
  - 默认：`info`
- `file`
  - 含义：可选日志文件路径。
  - 不配置时仅输出到标准输出。
  - 配置后，OxiDNS 会同时输出到标准输出和日志文件。
  - 日志文件内容为 UTF-8 纯文本格式，不写入终端 ANSI 颜色控制码。
- `rotation`
  - 含义：日志文件轮转策略。
  - 默认：`never`

`rotation` 支持以下配置：

- `type: never`
  - 不轮转，始终写入同一个文件。
- `type: minutely`
  - 按分钟轮转。
- `type: hourly`
  - 按小时轮转。
- `type: daily`
  - 按天轮转。
- `type: weekly`
  - 按周轮转。
  - 可选配置 `max_files`，表示最多保留多少个历史文件；`0` 表示不自动删除。

### `network`

`network.outbound` 用于集中管理项目内部 HTTP client 与 upstream 出站策略。未配置时保持兼容行为：HTTP client 使用系统 DNS 解析并直连目标地址，upstream 保持自身配置。

```yaml
network:
  outbound:
    default: direct
    profiles:
      direct:
        resolver: system
        proxy: none
      remote:
        resolver:
          nameservers:
            - addr: "1.1.1.1:53"
            - addr: "tls://dns.google:853"
              dial_addr: 8.8.8.8
            - addr: "https://cloudflare-dns.com/dns-query"
              dial_addr: 1.1.1.1
          ip_version: 4
          timeout: 5s
          proxy: none
        proxy:
          socks5: 127.0.0.1:1080
```

字段说明：

- `outbound.default`
  - 含义：未显式配置 `outbound` 的 HTTP client 和 upstream 默认使用哪个 profile。
  - 默认：无；无默认 profile 时使用系统 DNS + 直连。
  - 限制：如果配置，必须引用 `profiles` 中存在的名称。
  - 注意：默认 profile 的 proxy 会严格应用到 upstream；如果默认 SOCKS5 proxy 遇到 UDP、DoQ 或 DoH3 upstream，启动会失败，因为这些连接模型不支持 profile proxy。
- `outbound.profiles.<name>.resolver`
  - `system`：使用系统 DNS。HTTP client 中该解析是异步执行，不会阻塞运行时工作线程。
  - `nameservers`：使用指定 DNS nameserver 解析目标域名。支持 `udp://`、`tcp://`、`tls://`、`https://`、`doh://`、`h3://`、`quic://`、`doq://`；未写协议时按 UDP 处理。
  - 协议 feature：UDP/TCP 总是可用；DoT 需要 `resolver-dot`，DoH 需要 `resolver-doh`，DoQ 需要 `resolver-doq`，DoH3 需要 `resolver-doh3`。旧的 `upstream-*` feature 仍会启用共享 DNS client 依赖以兼容既有构建脚本，但新配置建议显式启用 `resolver-*`。
  - `ip_version`：可选，`4` 查询 A 记录，`6` 查询 AAAA 记录；未配置时默认 IPv4。
  - `timeout`：可选，resolver 查询超时，默认 `5s`。
  - `proxy`：可选，`none` 表示 nameserver 直连，`profile` 表示 TCP/DoT/DoH nameserver 复用当前 profile 的 SOCKS5。UDP/DoQ/DoH3 nameserver 不支持 SOCKS5。
  - 域名型 nameserver 必须配置 `dial_addr`，`addr` 中的域名用于 SNI/证书校验，`dial_addr` 用于实际连接，避免 resolver 解析自身。
- `outbound.profiles.<name>.proxy`
  - `none` 或 `direct`：直连。
  - `socks5`：通过 SOCKS5 代理连接目标地址，格式与上游 `socks5` 一致。

当前 `download`、`upgrade`、`http_request` 可通过 `args.outbound: remote` 引用 profile。旧字段 `socks5` 继续兼容；当同一个插件同时配置 `outbound` 和 `socks5` 时，`socks5` 会覆盖 profile 中的代理设置，但 resolver 仍来自该 outbound profile。`forward` upstream 未配置 `outbound` 时会使用 `network.outbound.default`；也可通过 `outbound: remote` 显式接入其他 profile。upstream 本地 `dial_addr`、`bootstrap`、`socks5` 优先于 profile 注入值。

### `api`

`api.http` 支持两种写法。

简写：

```yaml
api:
  http: "127.0.0.1:9088"
```

详写：

```yaml
api:
  http:
    listen: "127.0.0.1:9443"
    ssl:
      cert: "/etc/oxidns/api.crt"
      key: "/etc/oxidns/api.key"
      client_ca: "/etc/oxidns/client-ca.crt"
      require_client_cert: true
    auth:
      type: basic
      username: "admin"
      password: "secret"
    webui:
      root: "/etc/oxidns/webui"
      index: "index.html"
```

字段说明：

- `http.listen`
  - API 监听地址，支持 `ip:port`、`[ipv6]:port` 和 `:port`。
  - `:port` 会绑定为双栈 `[::]:port`；仅监听 IPv4 时请显式写 `0.0.0.0:port`。
- `http.ssl.cert`
  - API 证书文件。
- `http.ssl.key`
  - API 私钥文件。
- `http.ssl.client_ca`
  - 可选客户端证书 CA。
- `http.ssl.require_client_cert`
  - 是否要求双向 TLS。
- `http.auth`
  - 当前支持 `basic`。
  - Basic Auth 的请求头编码方式见《管理 API》章节。
- `http.cors.allowed_origins`
  - 可选的 WebUI/API 跨域白名单；未配置时会根据 `http.listen` 自动推导。
  - `0.0.0.0` 和 `[::]` 自动允许任意 origin；具体 IP 自动允许同一 host 的任意 WebUI 端口。
  - 显式配置时按浏览器 `Origin` 精确匹配。
  - 使用 `"*"` 可允许任意 origin，但不能与浏览器凭据跨域一起使用。
- `http.webui.root`
  - 可选的 WebUI 静态文件目录。启用后 WebUI 挂载在 `/`，管理 API 位于 `/api/*`。
  - 相对路径以 `-d/--working-dir` 为基准；例如 Debian service 默认 `-d /var/lib/oxidns`，因此 `root: "./webui"` 表示 `/var/lib/oxidns/webui`。
  - WebUI 构建、发布目录和 nginx 独立部署方式见《[WebUI 部署](../webui.md)》。
- `http.webui.index`
  - 可选首页文件名，默认 `index.html`。

校验规则：

- `listen` 不能为空。
- `cert` 和 `key` 必须成对出现。
- `require_client_cert: true` 时必须提供 `client_ca`。
- `basic.username` 和 `basic.password` 都不能为空。
- `webui.root` 不能为空。
- `webui.index` 配置后不能为空。

### `plugins`

每个插件定义都采用统一结构：

```yaml
- tag: cache_main
  type: cache
  args:
    size: 4096
```

通用规则：

- `tag`
  - 插件实例唯一标识。
  - 不能为空。
  - 在整个配置中必须唯一。
- `type`
  - 插件类型名。
  - 必须与已注册插件工厂一致。
- `args`
  - 插件参数。
  - 不同插件的参数形态不同，可能是对象、字符串、数组或空值。
