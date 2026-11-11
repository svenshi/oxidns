---
title: 运维与故障排查
---

本章面向已经部署 OxiDNS 的管理员，给出上线前检查、安全变更、健康判断和故障定位的推荐顺序。命令的完整参数见[命令行工具](cli.md)，接口字段见[管理 API](api.mdx)。

## 先记录部署基线

每个实例至少应记录以下信息：

| 项目 | 示例 |
| --- | --- |
| 版本与 bundle | `oxidns 1.5.1`、`full` |
| 安装方式 | release archive、Debian 包、Docker、OpenWrt |
| 配置路径 | `/etc/oxidns/config.yaml` |
| 工作目录 | `/var/lib/oxidns` |
| DNS 监听 | `:53` UDP/TCP、DoT/DoH/DoQ 地址 |
| 管理面 | API 地址、TLS、认证方式、WebUI 根目录 |
| 持久化数据 | cache dump、SQLite、provider 文件、日志 |
| 外部联动 | ipset、nftset、RouterOS 目标与 ownership prefix |

相对路径统一以 `-d/--working-dir` 为基准。配置路径相同但工作目录不同，会导致规则文件、日志、SQLite、WebUI 和升级目录指向不同位置，因此工作目录也是部署契约的一部分。

## 上线前检查

在启动、重载或升级前执行：

```bash
oxidns --version
oxidns build-info
oxidns check -c /etc/oxidns/config.yaml -d /var/lib/oxidns --graph
```

重点确认：

- `build-info` 中包含配置使用的协议和插件；slim bundle 不一定包含完整能力。
- 配置检查使用的 `-d` 与服务实际启动参数一致。
- 依赖图中的入口、matcher、executor 和 provider 引用符合预期。
- 监听端口没有被 systemd-resolved、dnsmasq、AdGuard Home 或其它 DNS 服务占用。
- TLS 证书、私钥、规则文件和持久化目录对服务用户可读写。

需要前台诊断时，可在停止现有服务后运行：

```bash
oxidns start -c /etc/oxidns/config.yaml -d /var/lib/oxidns -l debug
```

不要让第二个前台实例与生产实例同时绑定相同端口。

## 健康检查怎么判断

启用管理 API 后，可以检查：

```bash
curl -fsS http://127.0.0.1:9199/api/healthz
curl -fsS http://127.0.0.1:9199/api/readyz
curl -fsS http://127.0.0.1:9199/api/health
curl -fsS http://127.0.0.1:9199/api/build
```

受保护的部署应改用 HTTPS 和配置的认证方式，避免把长期凭据直接写入共享 shell 历史。

| 接口 | 能证明什么 | 不能证明什么 |
| --- | --- | --- |
| `/api/healthz` | 管理 API listener 已建立 | DNS 插件已经可用 |
| `/api/readyz` | 插件初始化完成且 server 已启动 | 每个外部上游都健康 |
| `/api/health` | 版本、bundle、实例、插件和启动状态 | 一次真实 DNS 查询一定成功 |
| `/api/build` | 当前二进制编译进的能力 | 当前配置已经正确应用 |

编排系统应使用 `healthz` 作为 liveness、`readyz` 作为 readiness。未编译 API 的版本需要结合进程状态和实际 DNS 探测判断。

## 安全修改配置

推荐使用以下顺序：

1. 备份当前配置和相关 provider/持久化文件，记录当前版本或哈希。
2. 尽量在独立候选文件中编辑，不直接覆盖线上文件。
3. 使用与服务相同的工作目录运行 `oxidns check`。
4. 查看 `--graph` 输出，确认依赖和入口没有意外变化。
5. 原子替换配置，或通过带版本检查的配置 API 保存。
6. 请求 reload；已有 reload 运行时不要循环重试。
7. 等待 `/api/reload/status` 完成，再检查 readiness、DNS 查询和关键指标。
8. 保留上一份配置直到观察窗口结束。

只更新规则数据时，优先使用 provider 级 reload；插件拓扑、全局配置或 server 发生变化时再使用全量 reload。全量 reload 失败会尝试恢复旧 runtime，但重建窗口仍可能产生短暂中断。

## 故障定位顺序

按下面顺序检查，避免一次修改多个层面：

1. 确认进程或系统服务是否运行、是否反复重启。
2. 确认版本、bundle、配置路径和工作目录。
3. 检查 API liveness、readiness 和 `/api/health`。
4. 从启动或 reload 日志中找到第一条因果错误。
5. 在本机对实际 DNS listener 发起查询。
6. 使用 `oxidns probe upstream` 单独验证受影响上游。
7. 检查 inflight、timeout、cache、forward 和外部联动指标。
8. 与上一份可用配置、二进制和运行基线比较。
9. 每次只恢复一个层面并重新验证。

### API 正常但 DNS 未就绪

- 确认至少存在一个 server 插件，且 `entry` 引用了可初始化的 executor。
- 检查监听地址冲突、低端口权限和 TLS 文件权限。
- 使用 `/api/build` 确认当前 bundle 包含需要的 server/协议。
- 不要只凭 `/healthz` 判断 DNS 已经可用。

### Listener 可访问但查询失败

- 先测试 hosts、response 等本地/合成应答，再测试需要上游的域名。
- 对每个上游使用相同的 outbound、bootstrap、SOCKS5 和 TLS 设置执行 `oxidns probe upstream`。
- 检查 forward timeout/error、fallback 启动和 resolver 错误。
- 排查默认 DNS 指向 OxiDNS 自身造成的 bootstrap 或转发环路。

### 延迟突然升高

- 分别观察 cache hit、cache miss 和上游路径，不要只看总平均值。
- 检查 inflight、上游延迟、连接池、timeout 和队列丢弃。
- 确认是否临时打开了 debug/trace、query recorder、script、HTTP 回调或同步外部联动。
- 在没有证据前不要先增加 worker 数和 timeout；这可能只会掩盖排队问题。

### Cache 行为异常

- 检查 hit、miss、expired、skip、lazy refresh 和 entry count 指标。
- 核对 QTYPE/QCLASS、ECS、负缓存和 TTL 配置。
- 清空或导入 dump 前先保留问题现场；这些操作应记录为运维事件。

### ipset、nftset 或 RouterOS 降级

- 先确认 DNS 主响应是否正常，再单独评估副作用链路。
- 检查 queue drop、reconnect、backoff、sync error 和 degraded 指标。
- 核对权限、凭据、TLS、ownership prefix 和目标集合/路由表。
- 不要批量删除不属于 OxiDNS ownership namespace 的条目。

## 升级与回滚

风险较高的环境建议分阶段升级：

```bash
oxidns upgrade check
oxidns upgrade download
sudo oxidns upgrade apply --no-restart
```

应用前确认目标平台、bundle、磁盘空间、WebUI 路径和目录权限，并单独备份配置与持久化数据。升级模块产生的二进制备份不能替代配置、SQLite 和 provider 数据备份。

替换完成后，根据安装方式显式启动或重启服务，再进行下面的验证。

升级后依次验证：

1. `oxidns --version` 与 `oxidns build-info`。
2. 服务没有进入重启循环。
3. `readyz` 返回成功。
4. 一次本地/合成查询和一次真实上游查询。
5. WebUI、管理 API、日志和关键指标。

需要回滚时，停止服务，恢复匹配的旧二进制、WebUI 和配置，再重复同一组健康与 DNS 检查。验证完成前不要删除升级备份。

## 提交问题前保留的信息

公开反馈前请准备：

- OxiDNS 版本、bundle、平台和安装方式。
- 脱敏后的最小配置与工作目录参数。
- 受影响协议、查询示例和预期结果。
- 第一条因果错误日志，而不是只有最后一条重试错误。
- health/build 快照与相关指标变化。
- 已执行的探测命令和结果。

不要公开密码、token、TLS 私钥、私有域名、客户端地址或完整 DNS 查询历史。安全问题请按[安全加固与漏洞报告](security.md)中的私密渠道报告。
