---
title: 安全加固与漏洞报告
---

OxiDNS 通常运行在网关、服务器或家庭网络边缘，既处理敏感 DNS 元数据，也可能操作配置、升级文件和外部网络系统。本章给出部署加固基线；漏洞披露规则以仓库根目录的 [`SECURITY.md`](https://github.com/svenshi/oxidns/blob/main/SECURITY.md) 为准。

## 管理面默认只放在可信网络

- 优先把 API 监听在 `127.0.0.1`、管理 VLAN 或 VPN 地址。
- 远程访问应启用 TLS 和 Basic Auth，或放在具有强认证的反向代理后。
- 高敏感环境可使用 mTLS。
- 使用防火墙限制来源；不要把未认证的 `/api/*` 暴露到公网。
- WebUI 静态文件不受 API Basic Auth 保护，但所有 `/api/*` 请求仍会校验认证。
- CORS 不是访问控制。`allowed_origins` 只约束浏览器，不能阻止其它 HTTP 客户端。

示例：只允许本机管理：

```yaml
api:
  http:
    listen: "127.0.0.1:9199"
    auth:
      type: basic
      username: ${ADMIN_USER}
      password: ${ADMIN_PASS}
```

需要远程管理时，优先通过反向代理或 VPN 暴露这一地址，而不是直接监听所有接口。

## 保护配置、凭据和查询数据

以下内容应视为敏感数据：

- `config.yaml` 中的密码、token、代理和 RouterOS 凭据。
- TLS 私钥和客户端 CA。
- query recorder SQLite、运行日志和抓包文件。
- 私有域名、客户端地址、ECS 信息和本地 provider 规则。
- 升级缓存、备份二进制和 WebUI 目录。

建议：

- 使用 `${VAR}` 从受控环境注入敏感值，不把凭据提交到 Git。
- 限制配置、私钥、SQLite 和日志的文件权限。
- 为查询记录设置必要的 retention，不长期保存无业务价值的明细。
- 分享配置和日志前进行脱敏；不要只删除密码而保留内部域名和客户端地址。
- 备份应具有与原始数据相同的访问控制和生命周期。

## 使用最小权限运行

- 普通转发实例不应以 root 长期运行。
- 绑定 53 等低端口、操作 ipset/nftset、安装系统服务或同步路由时，只授予实际需要的权限。
- 不使用的入站协议、管理 API 和插件应从配置或自定义 bundle 中移除。
- 将 OxiDNS 工作目录与其它服务的可写目录分开。
- 容器部署避免不必要的 privileged 模式和宿主机目录挂载。

## 高风险插件

| 插件/能力 | 主要风险 | 建议 |
| --- | --- | --- |
| `script` | 执行外部命令 | 固定命令路径，限制参数和服务用户权限 |
| `http_request` | 向外发送 DNS 派生数据 | 限制目标、模板字段和日志内容 |
| `download` | 下载并覆盖本地文件 | 使用 HTTPS、受控目录和最小写权限 |
| `upgrade` | 替换二进制和 WebUI | 保护 API、校验目标 bundle、保留回滚备份 |
| `ipset` / `nftset` | 修改宿主机网络状态 | 使用专用集合并限制 capability |
| RouterOS 插件 | 修改外部地址列表或路由 | 使用专用账号、TLS 和 ownership namespace |
| `query_recorder` | 持久化 DNS 查询历史 | 限制访问、控制 retention、避免公开数据库 |

上线前应审查这些插件的失败策略、timeout、并发上限、目标路径和清理行为。

## 上游与 TLS

- 默认保持 TLS 证书验证；`insecure_skip_verify` 只用于有边界的临时诊断。
- 域名型 TLS/HTTPS 上游需要正确的 SNI/证书名称；使用 `dial_addr` 时仍保留域名做校验。
- bootstrap resolver 应使用可信、不会形成环路的地址。
- SOCKS5、远端 resolver 和 webhook 都属于出站信任边界，应纳入同一网络策略审查。
- 不要在公开日志中记录完整代理凭据、Authorization header 或 GitHub token。

## 升级与供应链

- 从官方 GitHub Releases、官方容器仓库或可审计的自定义构建获取产物。
- 自动升级会使用 release asset digest 校验下载内容；手动下载也应核对发布信息和摘要。
- 生产环境固定明确版本，不依赖滚动 `latest` 标签完成不可控升级。
- 升级前确认平台和 bundle，升级后验证版本、构建能力、DNS、API 和 WebUI。
- 配置、SQLite 和 provider 数据需要独立备份；二进制备份不覆盖这些数据。

## 私密报告漏洞

怀疑存在安全漏洞时，请不要创建公开 Issue，也不要在 Telegram 或 Discussions 中发送利用细节、私有 DNS 数据或凭据。

使用以下任一私密渠道：

- 邮件：`isvenshi@gmail.com`
- GitHub Security Advisory / Private Vulnerability Reporting（仓库启用时）

报告中请提供受影响版本、平台、release asset 或 commit、脱敏后的最小配置/复现步骤、影响边界，以及问题是否可被远程触发。仅测试你拥有或明确获准评估的系统。
