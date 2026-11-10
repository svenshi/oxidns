---
title: 执行链与控制流
---

本页说明四类插件如何组合，以及 `sequence` 的规则、Quick Setup 和内建控制流语义。

## 四类插件的职责

### `server`

作用：接收 DNS 请求并把请求送入某个执行器入口。

特点：

- 不负责复杂策略判断。
- 核心配置通常是监听地址、TLS 参数、入口执行器。

### `executor`

作用：执行动作。

典型动作包括：

- 查询上游
- 生成本地响应
- 缓存读写
- TTL 调整
- ECS 处理
- 回退和并发竞争
- 观测与系统联动

### `matcher`

作用：做条件判断，供 `sequence` 规则使用。

典型判断维度包括：

- 查询域名
- 查询类型
- 客户端 IP
- 应答 IP
- 应答码
- 环境变量
- 采样命中
- 限流状态

### `provider`

作用：提供可复用规则集，供 `matcher` 或其它插件引用。

当前主要有：

- `domain_set`
- `ip_set`
- `geoip`
- `geosite`
- `adguard_rule`

## sequence 编排模型

`sequence` 是 OxiDNS 的策略中枢。绝大多数非平凡配置都会以它作为总入口。

示例：

```yaml
- tag: seq_main
  type: sequence
  args:
    - matches:
        - "$lan_clients"
        - "qtype A,28"
      exec: "$cache_main"
    - matches: "!has_resp"
      exec: "$forward_main"
    - exec: "accept"
```

每条规则支持两个核心字段：

- `matches`
  - 一个 matcher 表达式或表达式数组。
  - 数组中的所有条件都成立时，本条规则才命中。
- `exec`
  - 命中后执行的动作。

## 引用插件与 quick setup

### 引用已有插件

使用 `$tag` 引用已定义插件：

```yaml
- exec: "$forward_main"
- matches:
    - "$is_internal"
    - "!has_resp"
  exec: "$cache_main"
```

### quick setup

如果 `sequence` 中写的不是 `$tag`，而是 `type + 参数` 形式，OxiDNS 会即时构造临时插件。

示例：

```yaml
- exec: "forward 1.1.1.1 8.8.8.8"
- matches: "qname domain:example.com"
  exec: "ttl 300"
```

当前常见 quick setup：

- matcher
  - `_true`
  - `_false`
  - `qname ...`
  - `qtype ...`
  - `qclass ...`
  - `client_ip ...`
  - `resp_ip ...`
  - `ptr_ip ...`
  - `cname ...`
  - `mark ...`
  - `env ...`
  - `random ...`
  - `rate_limiter ...`
  - `rcode ...`
  - `has_resp`
  - `has_wanted_ans`
  - `string_exp ...`
- executor
  - `forward ...`
  - `cache ...`
  - `ttl ...`
  - `prefer_ipv4`
  - `prefer_ipv6`
  - `sleep ...`
  - `debug_print ...`
  - `query_summary ...`
  - `metrics_collector ...`
  - `black_hole ...`
  - `drop_resp`
  - `ecs_handler ...`
  - `forward_edns0opt ...`
  - `ipset ...`
  - `nftset ...`
  - `upgrade ...`
  - `download ...`
  - `reload_provider ...`
  - `reload`

## sequence 内建控制流

除了调用插件，`sequence.args[].exec` 还可以直接写内建控制流：

### `accept`

- 立即结束当前 `sequence`。
- 这是一次明确的提前停止，因此调用方不会继续执行后续规则。
- 不会自动生成响应。
- 典型用法：
  - `cache`、`hosts`、`arbitrary` 等前置 executor 已经写入 response 后，直接收口。
  - 命中某个分支后明确不希望再进入后续 `forward` / 副作用逻辑。

### `return`

- 立即结束当前 `sequence`，把控制权交回调用方。
- 不会自动生成响应。
- 如果当前 `sequence` 是被 `jump` 调用的，调用方会从 `jump` 后一条规则继续执行。
- 如果当前 `sequence` 是顶层入口，它等价于“提前结束当前规则链”。

### `reject [rcode]`

- 立即基于当前 request 构造一个 DNS 响应，并结束当前 `sequence`。
- 默认 `rcode` 为 `REFUSED`，所以 `reject` 等价于拒绝请求。
- 可以显式写十进制数值或英文 RCODE 名称；英文名称大小写不敏感。常见映射与含义见 [DNS 编码速查表](../dns-codes.md#rcode-响应码)，例如：
  - `reject 2` => `SERVFAIL`
  - `reject SERVFAIL` / `reject servfail` => `SERVFAIL`
  - `reject 3` => `NXDOMAIN`
  - `reject NXDOMAIN` => `NXDOMAIN`
- `reject` 只支持基础 DNS RCODE `0..15`；扩展 RCODE 需要 EDNS OPT，不会由该内建动作自动生成。
- `reject 0` 只返回普通 `NOERROR` 响应，不会自动附加 SOA。
- 调用方不会继续执行后续规则。
- 典型用法是直接返回指定错误码，例如：

```yaml
- matches: "qtype HTTPS"
  exec: "reject NXDOMAIN"
```

### `mark ...`

- 向 `DnsContext.marks` 追加一个或多个无符号整数 mark，保留集合中已有的值。
- 支持写法：
  - `mark 1`
  - `mark 1 2 3`
  - `mark 1,2,3`
- 写入后会继续执行当前 `sequence` 的下一条规则。
- 它本身不会生成响应，也不会终止当前 `sequence`。

### `set_mark ...`

- 用一个或多个无符号整数完整替换 `DnsContext.marks`，不会保留集合中原有的值。
- 参数语法与 `mark` 一致：
  - `set_mark 1`
  - `set_mark 1 2 3`
  - `set_mark 1,2,3`
- 重复值会自动去重；mark 是集合，配置顺序没有运行时语义。
- 至少需要一个值。缺少参数、负数、非数字或超出 `u32` 范围的值会导致 sequence 初始化失败。
- 替换后会继续执行当前 `sequence` 的下一条规则，不生成响应，也不终止当前 `sequence`。
- `set_mark` 替换整个集合，不区分“分类 mark”和“附加 mark”。如果需要保留某个值，必须把它明确写进新集合。

例如：

```yaml
- exec: "mark 1,4"
- exec: "set_mark 2,3"
- exec: "mark 5"
```

最终 marks 为 `2,3,5`：`set_mark` 移除了已有的 `1,4`，后续 `mark` 再追加 `5`。

### `jump seq_tag`

- 调用另一个 `sequence`，语义上类似“子过程调用”。
- 参数必须是目标 `sequence` 的 tag，且不能写 `$` 前缀。
- 被调用的 `sequence` 如果：
  - 正常执行到尾部，当前 `sequence` 会从 `jump` 的下一条规则继续。
  - 中途执行了 `return`，当前 `sequence` 也会从 `jump` 的下一条规则继续。
  - 中途执行了 `accept`、`reject` 或其它返回 `Stop` 的操作，当前 `sequence` 也会一起停止，不再继续后续规则。

### `goto seq_tag`

- 直接把控制权转交给另一个 `sequence`，语义上类似“单向跳转”。
- 参数必须是目标 `sequence` 的 tag，且不能写 `$` 前缀。
- 当前 `sequence` 在执行 `goto` 后不会恢复：
  - 目标 `sequence` 正常跑到尾部，不回到 `goto` 后面的规则。
  - 目标 `sequence` 执行 `return`，该 `return` 会继续向外层传播，但同样不回到 `goto` 后面的规则。
  - 目标 `sequence` 执行 `accept` / `reject` / 其它 `Stop`，结果也直接向外层传播。
- 适合把请求永久移交给另一个策略分支。

示例：

```yaml
- matches: "$rate_ok"
  exec: "mark 100"
- matches: "!$rate_ok"
  exec: "reject 2"
```

`jump` / `goto` 的区别示例：

```yaml
- tag: child_seq
  type: sequence
  args:
    - exec: "set_mark 2,20"
    - exec: "return"

- tag: parent_jump
  type: sequence
  args:
    - exec: "mark 1"
    - exec: "jump child_seq"
    - exec: "mark 3"

- tag: parent_goto
  type: sequence
  args:
    - exec: "mark 1"
    - exec: "goto child_seq"
    - exec: "mark 3"
```

- `parent_jump` 最终会留下 `2,3,20`：子 sequence 与调用方共享同一个 `DnsContext`，因此 `set_mark` 会替换调用方先前写入的 `1`，随后父 sequence 继续追加 `3`。
- `parent_goto` 最终只会留下 `2,20`，因为 `set_mark` 同样替换了 `1`，且控制权不会回到 `goto` 之后。
