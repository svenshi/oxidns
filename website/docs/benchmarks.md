---
title: 性能测试
sidebar_position: 8
---

# 性能测试

本页展示 OxiDNS `oxidns 1.5.1 (full)`、mosdns `v5.3.4-0-gb732318`、AdGuard Home `AdGuard Home, version v0.107.78` 与 SmartDNS `smartdns 1.2026.06.28-1614 (Release48.2)` 的阶段性实测快照，dnsperf 版本为 `2.15.1`。仅在架构、关键请求路径、测试口径或重要里程碑发生明显变化时更新，不要求每个版本重复测试。

本轮数据采集于 `2026-07-24T19:07:42.209737+08:00`。测试参数见[完整原始报告](/benchmarks/staged/report.txt)。每个指标取多次重复的中位数；最大稳定吞吐只接受丢包率不高于 0.1% 的点。进程 CPU 的 100% 表示占满一个逻辑核。

## 被测环境

* CPU：`Model name:                              Intel(R) N100`，逻辑核 `4`
* 内存：`Mem:           512Mi        59Mi        49Mi        80Ki       403Mi       452Mi`
* OxiDNS：`oxidns 1.5.1 (full)`，SHA-256 `8cff1b81a6518f4436308750fb24700fd1389d747d171adaca809d1110e73518`
* mosdns：`v5.3.4-0-gb732318`，SHA-256 `5357fbb83c89f0a7acad275b72c33aa70d4c720cb5590525660132b10cee8af9`
* AdGuard Home：`AdGuard Home, version v0.107.78`，SHA-256 `fad50bcebf485fa3e8eec3c01db2dded54d02dd73bdab18a8dc79db6ba99b655`
* SmartDNS：`smartdns 1.2026.06.28-1614 (Release48.2)`，SHA-256 `2e51d85a70ab30002c83a36fcc5e1a3e62169e0b561bbd1e7508419a21fdb33e`
* dnsperf：`2.15.1`

## 规则格式与响应语义核验

* 域名集合只生成一次并由四款软件共同加载：去掉可安全映射的 `full:` 前缀，保留纯域名与 `domain:` 项，排除无法在四方保持等价的正则/关键字规则、无效名称和重复项。
* 规范化统计：`{"duplicate": 198, "included_full": 1247, "included_plain": 142119, "invalid_domain": 113, "normalized_unique_domains": 143366, "positive_query_domains": 24, "unsupported_regexp": 163}`。
* 计时前解析真实 DNS 响应：集合命中必须返回 `192.0.2.53`，固定未命中控制必须返回 `192.0.2.54`；证据保存在[语义断言](/benchmarks/staged/semantic-validation.json)。
* 正缓存与负缓存由本地确定性上游预热；每个计时区间的上游请求计数必须为 0，否则运行直接失败，不发布该结果。
* 响应 IP/CIDR 匹配不进入四引擎主矩阵：四款产品对应内存匹配、响应过滤或操作系统 ipset 副作用，单纯转换文本格式不能建立等价工作负载。
* 本轮共采集 **432 个真实计时样本**、形成 144 个三轮中位聚合点；24 组场景/引擎组合共通过 **72 个报文语义探针**。正负缓存的 144 个计时样本全部记录为零回源。
* [环境快照](/benchmarks/staged/environment.json)还保存了 36 个实际输入文件的 SHA-256，包括 runner、场景目录、四引擎配置、查询集、规则数据和生成后的规范化域名集合。

## 指标怎么看

* **QPS / 吞吐量：越高越好**，但前提是丢包率和尾延迟仍在可接受范围内。
* **p50、p95、p99、最大延迟：越低越好**。p99 表示 99% 已完成请求的响应时间不超过该值，比平均值更容易看出排队和长尾卡顿。
* **丢包率：越低越好**。本报告只有在丢包率中位数不超过 0.1% 时，才把该并发点计为“稳定”。
* **CPU：相同吞吐量下越低越好**。不能脱离 QPS 单看 CPU；如果使用更多 CPU 换来了明显更高吞吐，仍可能是合理结果。这里 100% 表示占满一个逻辑核。
* **RSS 内存：相同负载下越低越好**，表示测试过程中进程实际驻留在物理内存中的容量。
* 看折线图时，理想状态是并发增加后 QPS 继续上升，同时 p99 和丢包保持稳定；如果 QPS 已经走平而 p99 快速升高，说明服务已经进入饱和区。

## 一、四引擎通用性能矩阵

这一部分只比较四款软件都能保持相同输入和响应语义的路径。域名集合场景使用规范化纯域名并计时正命中；OxiDNS/mosdns 的原生规则专项作为独立区块保留，不与四引擎排名混算。

### 通用域名正命中

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/domain-matching.svg" alt="143,366 个域名的真实命中吞吐与容量保留率" />
  </div>
  <div className="col col--4">
    <p><strong>OxiDNS：122,941 QPS，约 103% 基线容量</strong></p>
    <p>在 143,366 域名真实正命中下，OxiDNS 分别领先 mosdns 16.0%、SmartDNS 54.1%、AdGuard Home 84.8%。相对自身本地回答基线的 103.3% 属于重复波动，客观含义是没有测出吞吐损失；代价主要是 RSS 增加约 17.7 MiB。</p>
  </div>
</div>

### 吞吐与并发扩展

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/throughput.svg" alt="各场景最大稳定吞吐量柱状图" />
  </div>
  <div className="col col--4">
    <p><strong>越高越好</strong></p>
    <p>OxiDNS 在五个 UDP 场景的最大稳定吞吐均最高；mosdns 在 TCP 最大完成吞吐领先。TCP 的 loss 门槛不会反映深队列，必须结合下一张 p99 图。</p>
  </div>
</div>

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/stable-tail-latency.svg" alt="各引擎最大稳定吞吐点的 p99" />
  </div>
  <div className="col col--4">
    <p><strong>在标注吞吐量下越低越好</strong></p>
    <p>UDP 稳定点的 OxiDNS p99 为 2.0–2.1 ms。TCP q1024 虽无丢失，但四引擎 p99 已达 16.9–55.3 ms，表示明显排队，不能把“零丢失”直接理解为推荐工作点。</p>
  </div>
</div>

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/scaling.svg" alt="并发扩展折线图" />
  </div>
  <div className="col col--4">
    <p><strong>上升且不过早走平更好</strong></p>
    <p>这张 UDP 基线曲线显示 OxiDNS 与 mosdns 能扩展到 q256；SmartDNS 约一个逻辑核后走平。q1024 四引擎丢包都超过 0.1%，因此只用于显示饱和。</p>
  </div>
</div>

### 尾延迟

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/tail-latency.svg" alt="p99 尾延迟折线图" />
  </div>
  <div className="col col--4">
    <p><strong>越低越好</strong></p>
    <p>UDP q256 的 p99 为 OxiDNS 2.05 ms、mosdns 2.43 ms、SmartDNS 2.11 ms、AdGuard Home 7.55 ms；q1024 丢包越线，因此不进入稳定容量。</p>
  </div>
</div>

### CPU 与内存

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/cpu.svg" alt="CPU 占用柱状图" />
  </div>
  <div className="col col--4">
    <p><strong>相同吞吐量下越低越好</strong></p>
    <p>五个 UDP 稳定点每万 QPS 的 CPU 成本约为 OxiDNS 17.1%–18.9%、mosdns 14.3%–16.0%、SmartDNS 11.4%–13.5%。OxiDNS用更多 CPU 换取最高容量，mosdns 与 SmartDNS 单位吞吐更省 CPU。</p>
  </div>
</div>

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/memory.svg" alt="RSS 内存柱状图" />
  </div>
  <div className="col col--4">
    <p><strong>相同负载下越低越好</strong></p>
    <p>SmartDNS 的稳定点 RSS 最低（6.0–22.2 MiB）；OxiDNS 为 13.6–31.4 MiB，低于 mosdns 的 22.8–41.9 MiB。AdGuard Home 为 49.6–88.7 MiB，在本矩阵中最高。</p>
  </div>
</div>

### 四引擎各场景最大稳定点

| 场景 | 引擎 | 并发 | QPS | p99 | CPU | RSS | 丢包 |
|---|---|---:|---:|---:|---:|---:|---:|
| 02-cache-hotpath | OxiDNS | 256 | 113,242.1 | 2.047 ms | 214.1% | 14.3 MiB | 0.0340% |
| 02-cache-hotpath | mosdns | 256 | 109,273.2 | 2.623 ms | 169.1% | 22.9 MiB | 0.0345% |
| 02-cache-hotpath | AdGuard Home | 256 | 65,555.6 | 7.935 ms | 269.0% | 51.8 MiB | 0.0290% |
| 02-cache-hotpath | SmartDNS | 64 | 78,353.9 | 0.927 ms | 99.8% | 6.3 MiB | 0.0000% |
| 47-server-local-udp | OxiDNS | 256 | 119,754.3 | 2.047 ms | 204.6% | 13.6 MiB | 0.0308% |
| 47-server-local-udp | mosdns | 256 | 111,019.1 | 2.431 ms | 159.3% | 22.8 MiB | 0.0303% |
| 47-server-local-udp | AdGuard Home | 256 | 72,285.7 | 7.551 ms | 258.7% | 50.0 MiB | 0.0250% |
| 47-server-local-udp | SmartDNS | 256 | 87,537.3 | 2.111 ms | 99.8% | 6.0 MiB | 0.0267% |
| 48-server-local-tcp | OxiDNS | 1024 | 151,822.6 | 16.895 ms | 249.5% | 14.7 MiB | 0.0000% |
| 48-server-local-tcp | mosdns | 1024 | 161,175.5 | 23.039 ms | 264.5% | 24.6 MiB | 0.0000% |
| 48-server-local-tcp | AdGuard Home | 1024 | 74,921.8 | 55.295 ms | 274.5% | 49.6 MiB | 0.0000% |
| 48-server-local-tcp | SmartDNS | 1024 | 91,752.7 | 20.479 ms | 99.8% | 6.3 MiB | 0.0000% |
| 50-common-local-answers | OxiDNS | 256 | 119,046.3 | 2.111 ms | 209.4% | 13.7 MiB | 0.0321% |
| 50-common-local-answers | mosdns | 256 | 111,199.5 | 2.431 ms | 159.7% | 22.9 MiB | 0.0341% |
| 50-common-local-answers | AdGuard Home | 256 | 70,829.2 | 7.679 ms | 256.5% | 50.5 MiB | 0.0254% |
| 50-common-local-answers | SmartDNS | 256 | 83,245.9 | 2.303 ms | 99.8% | 6.0 MiB | 0.0267% |
| 51-common-domain-set | OxiDNS | 256 | 122,941.0 | 2.015 ms | 209.6% | 31.4 MiB | 0.0311% |
| 51-common-domain-set | mosdns | 256 | 105,951.1 | 3.135 ms | 169.7% | 41.9 MiB | 0.0358% |
| 51-common-domain-set | AdGuard Home | 64 | 66,532.8 | 3.135 ms | 249.0% | 88.7 MiB | 0.0000% |
| 51-common-domain-set | SmartDNS | 64 | 79,781.2 | 0.911 ms | 99.8% | 22.2 MiB | 0.0000% |
| 52-negative-cache-hotpath | OxiDNS | 256 | 113,469.6 | 2.111 ms | 204.7% | 14.3 MiB | 0.0338% |
| 52-negative-cache-hotpath | mosdns | 256 | 107,298.8 | 2.687 ms | 169.0% | 22.8 MiB | 0.0351% |
| 52-negative-cache-hotpath | AdGuard Home | 256 | 63,017.5 | 8.703 ms | 267.9% | 52.2 MiB | 0.0277% |
| 52-negative-cache-hotpath | SmartDNS | 64 | 73,919.4 | 1.007 ms | 99.8% | 6.3 MiB | 0.0000% |

### 四引擎矩阵客观评价

* **OxiDNS 的优势集中在本地 UDP 容量与域名匹配，而不是所有指标。** 它在正缓存、UDP 本地回答、A/AAAA 本地覆盖、规范化域名集合和负缓存五项的最大稳定吞吐均最高；相对 mosdns 高 3.6%–16.0%，相对 SmartDNS 高 36.8%–54.1%，相对 AdGuard Home 高 65.7%–84.8%。对应代价是五项每万 QPS CPU 成本均高于 mosdns。
* **143,366 域名正命中是通用矩阵中最能体现 OxiDNS 特性的结果。** OxiDNS 达到 122,941.0 QPS、p99 2.015 ms、RSS 31.4 MiB。它相对自身 119,046.3 QPS 的本地回答基线保留 103.3% 容量；大于 100% 不代表查表会加速，而是说明三轮波动范围内没有测出额外吞吐成本。其域名索引 RSS 增量约 17.7 MiB；mosdns、SmartDNS、AdGuard Home 的容量保留率分别为 95.3%、95.8%、93.9%。
* **TCP 给出了真实的反向结果。** 按统一的 loss≤0.1% 规则，mosdns q1024 中位 161,175.5 QPS，比 OxiDNS 的 151,822.6 高 6.2%；OxiDNS p99 则低 26.7%（16.895 对 23.039 ms），RSS 低 40.2%。更重要的是，q1024 相对 q256 只增加约 13%–15% 吞吐，却把两者 p99 从 4.863/5.503 ms 推到 16.895/23.039 ms；mosdns、OxiDNS、AdGuard Home 该点三轮 QPS 变异系数分别为 19.8%、8.0%、28.0%。因此表中的 TCP q1024 是 loss 门槛下的最大完成吞吐，不是低延迟推荐工作点。
* **SmartDNS 的优势是资源占用与低并发效率。** 它的稳定点通常约占一个逻辑核，RSS 为 6.0–22.2 MiB；代价是 UDP 曲线较早走平。AdGuard Home 在这组关闭查询日志/统计的窄口径数据路径中，吞吐最低、CPU/RSS和p99通常最高；这不评价其管理界面、客户端策略和完整过滤产品能力。
* **除 TCP q1024 外，重复性足以支持本机阶段性比较。** 五个 UDP 场景的 20 个最大稳定点三轮 QPS 变异系数均不超过 4.46%。本轮不能压缩成一个“综合总冠军”，也不能外推成其他硬件、跨机流量或生产上游的固定倍数。

{/* native-specialized:start */}
## 二、OxiDNS 与 mosdns 等价配置专项

这一部分只比较 OxiDNS 与 mosdns，因为以下四条路径可以让两个引擎直接加载**同一份 YAML 配置、查询文件和原始规则文件**，不需要把 `domain_set` 或 `ip_set` 转换成另一种产品格式。专项数据采集于 `2026-07-24T16:47:08.506963+08:00`，版本、二进制 SHA-256 和服务器与第一部分一致；每个点为 3 次中位数，稳定点仍要求丢包率中位数不高于 0.1%。

* `08-domain-set`：直接加载相同的两份 geosite 文本，保留普通后缀、`full:` 和 `regexp:` 规则；计时流量混合 10 个命中与 8 个未命中。
* `09-ip-set`：直接加载相同的 64 条原始 CIDR 文件；4 个返回地址位于集合内，4 个位于集合外，分别断言 accept 和 SERVFAIL。
* `42-composite-local-rewrite`：使用相同的 redirect → arbitrary A/AAAA → TTL rewrite → accept/reject 处理链；源记录 TTL 为 300，报文断言输出 TTL 为 60。
* `43-composite-provider-chain`：使用相同的域名集合、合成回答、响应 IP 集合和 accept/reject 分支，测量完整 provider/matcher 组合链。

计时前双方都通过了上述 A、AAAA、RCODE 与 TTL 检查。每个引擎 39 个精确断言，共 **78 个语义探针**，原始报文解析结果见[专项语义断言](/benchmarks/staged/native-specialized-semantic-validation.json)。

### 四项最大稳定吞吐

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/native-specialized-throughput.svg" alt="OxiDNS 与 mosdns 四项等价配置专项最大稳定吞吐" />
  </div>
  <div className="col col--4">
    <p><strong>越高越好，但必须先通过丢包门槛</strong></p>
    <p>OxiDNS 在域名集合和 provider 组合链分别达到 mosdns 的 3.93 倍和 5.25 倍；IP 集接近持平。本地重写链的稳定吞吐由 mosdns 领先 11.7%。</p>
  </div>
</div>

### 最大稳定吞吐对应的 p99

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/native-specialized-stable-tail-latency.svg" alt="四项专项各自最大稳定吞吐点的 p99 尾延迟" />
  </div>
  <div className="col col--4">
    <p><strong>越低越好；需要和吞吐、并发一起看</strong></p>
    <p>在各自最大稳定吞吐点，OxiDNS 四项 p99 都更低；但延迟仍需与同一行的吞吐和并发结合判断，不能脱离容量单独排名。</p>
  </div>
</div>

### 原生域名集合的并发扩展

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/native-specialized-domain-scaling.svg" alt="原生 domain_set 吞吐随并发变化" />
  </div>
  <div className="col col--4">
    <p><strong>上升且不过早走平更好</strong></p>
    <p>并发 64 时为 133,302.5 对 36,361.0 QPS，并发 256 时为 142,929.5 对 36,226.3 QPS；优势分别是 3.67 倍和 3.95 倍。mosdns 在 64 后基本走平，双方 1024 并发都因丢包超过 0.1% 被排除。</p>
  </div>
</div>

### 原生域名集合的 p99 尾延迟

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/native-specialized-domain-tail-latency.svg" alt="原生 domain_set 的 p99 尾延迟" />
  </div>
  <div className="col col--4">
    <p><strong>越低越好</strong></p>
    <p>在双方都满足丢包门槛的 256 并发点，OxiDNS p99 为 1.663 ms，mosdns 为 11.775 ms；OxiDNS 吞吐为 3.95 倍，而 p99 低 85.9%。</p>
  </div>
</div>

### 专项 CPU 与内存

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/native-specialized-cpu.svg" alt="四项专项最大稳定吞吐点 CPU" />
  </div>
  <div className="col col--4">
    <p><strong>结合 QPS 看；相同吞吐下越低越好</strong></p>
    <p>域名集合和 provider 链中，OxiDNS 在明显更高吞吐下 CPU 仍更低；IP 集和本地重写链中 OxiDNS CPU 更高。</p>
  </div>
</div>

<div className="row benchmark-chart-panel">
  <div className="col col--8">
    <img src="/img/benchmarks/staged/native-specialized-memory.svg" alt="四项专项最大稳定吞吐点 RSS" />
  </div>
  <div className="col col--4">
    <p><strong>相同场景下越低越好</strong></p>
    <p>OxiDNS 四项 RSS 均更低，为 13.8–35.1 MiB；mosdns 为 22.6–43.6 MiB。各场景降幅约 19%–39%，但内存优势不应替代吞吐与尾延迟判断。</p>
  </div>
</div>

### 专项最大稳定点

| 场景 | 引擎 | 并发 | QPS | p99 | CPU | RSS | 丢包 |
|---|---|---:|---:|---:|---:|---:|---:|
| 08-domain-set | OxiDNS | 256 | 142,929.5 | 1.663 ms | 194.2% | 34.6 MiB | 0.0268% |
| 08-domain-set | mosdns | 64 | 36,361.0 | 4.735 ms | 329.4% | 43.4 MiB | 0.0000% |
| 09-ip-set | OxiDNS | 256 | 117,380.3 | 2.111 ms | 201.8% | 14.2 MiB | 0.0327% |
| 09-ip-set | mosdns | 256 | 112,325.5 | 2.495 ms | 159.5% | 22.6 MiB | 0.0336% |
| 42-composite-local-rewrite | OxiDNS | 256 | 98,005.1 | 2.303 ms | 219.3% | 13.8 MiB | 0.0391% |
| 42-composite-local-rewrite | mosdns | 256 | 109,458.2 | 2.431 ms | 163.7% | 22.6 MiB | 0.0348% |
| 43-composite-provider-chain | OxiDNS | 256 | 136,728.5 | 1.823 ms | 194.4% | 35.1 MiB | 0.0280% |
| 43-composite-provider-chain | mosdns | 64 | 26,063.1 | 6.271 ms | 344.2% | 43.6 MiB | 0.0000% |

### 专项客观评价

* **OxiDNS 的原生域名匹配优势明确且跨负载存在。** 在并发 1、4、16、64、256 的稳定范围内，OxiDNS 三轮中位吞吐为 mosdns 的约 **3.67–4.36 倍**。最大稳定吞吐为 **3.93 倍**；在相同的 256 并发下仍为 **3.95 倍**，同时 p99 低 85.9%，所以结论不是由挑选不同并发点造成。
* **完整 provider/matcher 链是差距最大的路径。** 最大稳定吞吐为 **5.25 倍**。mosdns 在 256 并发的丢包中位数为 `0.1029%`，略超统一阈值；即使只比较双方零丢包的并发 64，OxiDNS 仍为 **4.91 倍**，p99 为 1.311 对 6.271 ms。
* **IP 集结果应评价为接近，而不是宣称明显领先。** 256 并发中 OxiDNS QPS 高 4.5%、p99 低 15.4%、RSS 低 37.0%，但 CPU 高 26.5%；而且 OxiDNS 该点三轮 QPS 变异系数为 5.95%，高于 4.5% 的吞吐差，因此小幅领先不足以视作稳定的数量级优势。
* **本地重写链显示了真实的反向结果。** mosdns 吞吐高 11.7%、CPU 低 25.3%，OxiDNS p99 低 5.3%、RSS 低 39.0%；这说明专项不是为了让 OxiDNS 在每一项都领先。
* **样本重复性整体可用，但不等于跨机器结论。** 除上述 OxiDNS IP 集点外，其余 7 个最大稳定点三轮 QPS 变异系数均不超过 2.1%。这些数据适合做同机阶段性工程对比，不能外推成所有 DNS 工作负载的固定倍数，也不代表冷加载、规则热更新、上游转发、加密协议或独立压测机下的容量。
* **四引擎矩阵和两引擎专项不能合并排名。** 第一部分比较四款软件都能等价表达的规范化路径；本专项只在 OxiDNS 与 mosdns 间保留原生规则种类、未命中遍历和完整策略链。两部分都是真实实测，但回答的是不同问题。

专项原始资料：[报告](/benchmarks/staged/native-specialized-report.txt)、[聚合 TSV](/benchmarks/staged/native-specialized-summary.tsv)、[144 个逐轮样本](/benchmarks/staged/native-specialized-summary.raw.json)、[78 个语义探针](/benchmarks/staged/native-specialized-semantic-validation.json)、[环境快照](/benchmarks/staged/native-specialized-environment.json)。
{/* native-specialized:end */}

## 代表性判断

本矩阵对**四款软件都能保持相同可观察语义的稳定本地 UDP/TCP 请求路径**具有代表性：最小监听器、本地回答、正/负热缓存和真实域名集合查询被分开测试，并通过多档并发展示扩展、饱和与排队，而不是只比较一个峰值 QPS。

它不能代表冷启动/热重载、以缓存未命中为主的转发、DoT/DoH/DoQ、公网上游质量、跨机网络开销、DNSSEC 验证，或 ipset/nftset 等宿主机副作用。响应 IP/CIDR 匹配也未放入四引擎主矩阵，因为四款产品对应的是内存响应匹配、响应过滤或操作系统 ipset 副作用等不同语义，不能仅靠转换文件格式就宣称等价。生产容量测试还应使用独立压测机，并为这些路径建立单独矩阵。

## 口径限制

本轮是同机 loopback 对比，适合观察本地请求路径成本、并发扩展和排队，不代表其他硬件上的生产容量，也不把公网转发上游波动混进默认结论。可下载：[完整报告](/benchmarks/staged/report.txt)、[聚合 TSV](/benchmarks/staged/summary.tsv)、[逐轮 JSON](/benchmarks/staged/summary.raw.json)、[语义断言](/benchmarks/staged/semantic-validation.json)、[环境快照](/benchmarks/staged/environment.json)。
