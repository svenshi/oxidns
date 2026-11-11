---
title: 升级命令
---


本页说明 release 检查、下载和应用。生产升级前请备份二进制、WebUI、配置和持久化数据，并准备回滚。

## `upgrade`

检查、下载或应用 GitHub Release 中的 OxiDNS 升级包。

支持以下子命令：

- `upgrade check`
- `upgrade download`
- `upgrade apply`

典型用法：

```bash
oxidns upgrade
oxidns upgrade --force
oxidns upgrade check
oxidns upgrade download --target latest
sudo oxidns upgrade apply
sudo oxidns upgrade apply --no-restart
```

通用参数：

- `--target <TAG|latest>`
  - Release tag 或 `latest`。
  - 默认值：`latest`
- `--repository <OWNER/REPO>`
  - GitHub 仓库。
  - 默认值：`svenshi/oxidns`
- `--asset <NAME|auto>`
  - Release asset 名称；`auto` 会按当前平台和编译版本选择 archive。
  - 默认值：`auto`
- `-c, --config <PATH>`
  - 运行配置文件路径，用于在未显式指定 `--webui-dir` 时读取 `api.http.webui.root`。
  - 未指定时，优先使用当前目录的 `config.yaml`；Linux 打包安装环境中，如果存在 `/etc/oxidns/config.yaml`，会用它推导 WebUI 路径。
- `-d, --working-dir <DIR>`
  - 运行期相对路径的基准目录，语义与 `start -d/--working-dir` 一致。
  - 未指定且检测到 Linux 打包配置时，默认使用 `/var/lib/oxidns`；否则使用当前目录。
- `--bundle <auto|full|standard|minimal>`
  - 当 `--asset auto` 时选择 release 编译版本。
  - 默认值：`auto`，跟随当前二进制的编译版本。
  - `full` 使用旧资产名，例如 `oxidns-x86_64-unknown-linux-musl.tar.gz`；`standard` / `minimal` 使用 slim 资产名，例如 `oxidns-standard-x86_64-unknown-linux-musl.tar.gz`。
- `--cache-dir <DIR>`
  - 升级文件缓存目录。
  - 默认值：`./upgrade-cache`
- `--backup-dir <DIR>`
  - `apply` 替换前的二进制备份目录。
  - 默认值：`./upgrade-backups`
- `--webui-dir <DIR>`
  - `apply` 时安装 WebUI 静态资源的目录；相对路径按 `-d/--working-dir` 解析，应与 `api.http.webui.root` 一致。
  - 未指定时，优先从运行配置的 `api.http.webui.root` 推导；没有配置时使用 `./webui`。
- `--skip-webui`
  - `apply` 时跳过 WebUI 目录升级，仅替换二进制文件。
- `--no-restart`
  - `apply` 成功后跳过服务重启。默认会通过系统服务管理器（systemd / launchd / Windows SCM）自动重启已安装的服务。
- `--allow-prerelease`
  - 允许使用 prerelease。
- `--force`
  - `apply` 时即使目标 release 不比当前版本更新，也继续下载、校验并替换。
- `--timeout <DURATION>`
  - HTTP 请求超时，例如 `30s`、`2m`。
- `--socks5 <ADDR>`
  - 可选 SOCKS5 代理。
- `--insecure-skip-verify`
  - 跳过 TLS 证书校验。
- `--github-token <TOKEN>`
  - GitHub 个人访问令牌，用于提高 API 速率限制或访问私有仓库。

行为说明：

- `check` 只查询 release 并判断版本是否更新。
- `download` 下载 archive，并使用 GitHub release asset 的 `digest` 字段校验 SHA256。
- 显式传入 `--asset` 时优先使用指定 asset，不再根据 `--bundle` 推导。
- 不写子命令时默认执行 `apply`。
- `apply` 默认只有检测到新版本才会更新；`--force` 会强制更新。
- `apply` 在 Unix 平台会解包 `.tar.gz`、备份当前二进制并替换；Windows 会解包 `.zip`、备份并替换二进制，同样支持 WebUI 目录升级。
- `apply` 默认在替换二进制后，将 archive 中的 `webui/` 目录备份并安装到 `--webui-dir`；`--skip-webui` 可跳过；archive 不含 `webui/` 时跳过且不影响二进制升级。
- Debian 打包安装的默认布局中，直接运行 `sudo oxidns upgrade apply` 会按 `/etc/oxidns/config.yaml` 和 `/var/lib/oxidns` 推导 WebUI 目录；`/var/lib/oxidns/webui` 是符号链接时，会更新其指向的真实目录。
- `apply` 成功后默认通过系统服务管理器重启服务；如果不想自动重启，传 `--no-restart`。
- `apply` 成功后会询问是否清理缓存目录和备份目录，默认选择 `Y`。
