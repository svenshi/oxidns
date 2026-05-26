---
title: 快速开始
sidebar_position: 2
---

本页介绍 OxiDNS 的安装方式，并给出从下载到首次启动的最短路径。

仅需快速验证时，优先使用以下方式：

- Linux 服务器：优先使用一条命令安装脚本、release 压缩包或 `.deb`
- 容器环境：优先使用 GHCR Docker 镜像
- 需要自行改代码或调试：使用 Cargo 从源码构建
- macOS / Windows：优先使用一条命令安装脚本或 release 压缩包

## 1. 一条命令安装

安装脚本会自动识别当前平台，下载匹配的 GitHub Release archive，解压 `oxidns` / `oxidns.exe`、`config.yaml` 和 `webui/`，并默认安装、启动为系统服务。

Linux / macOS：

```bash
curl -fsSL https://oxidns.org/install.sh | sudo sh
```

Windows 管理员 PowerShell：

```powershell
irm https://oxidns.org/install.ps1 | iex
```

默认安装位置：

| 平台 | 默认安装目录 | PATH 行为 |
| --- | --- | --- |
| Linux / macOS `sudo` 服务安装 | `/opt/oxidns` | 在 `/usr/local/bin/oxidns` 创建链接，并安装、启动系统服务 |
| Windows 管理员 PowerShell 服务安装 | `%ProgramFiles%\OxiDNS` | 加入 Machine PATH，并安装、启动系统服务 |
| Linux / macOS 便携安装 | `~/.oxidns` | 设置 `OXIDNS_INSTALL_SERVICE=0` 后，在 `~/.local/bin/oxidns` 创建链接 |
| Windows 便携安装 | `%LOCALAPPDATA%\OxiDNS` | 设置 `OXIDNS_INSTALL_SERVICE=0` 后，将安装目录加入用户 PATH |

重复运行安装脚本会更新二进制和 `webui/`。如果安装目录中已经有 `config.yaml`，脚本会保留现有配置，并把 release 内置配置写成 `config.yaml.example`。

安装指定版本：

```bash
curl -fsSL https://oxidns.org/install.sh | sudo env OXIDNS_VERSION=v1.0.1 sh
```

```powershell
$env:OXIDNS_VERSION = "v1.0.1"; irm https://oxidns.org/install.ps1 | iex
```

常用环境变量：

| 变量 | 作用 |
| --- | --- |
| `OXIDNS_VERSION` | release 标签，默认 `latest` |
| `OXIDNS_INSTALL_DIR` | 覆盖安装目录 |
| `OXIDNS_BIN_DIR` | Linux / macOS 下覆盖命令链接目录 |
| `OXIDNS_TARGET` | 手动指定 release target，例如 `x86_64-unknown-linux-musl` |
| `OXIDNS_NO_PATH=1` | 不创建命令链接或不修改 PATH |
| `OXIDNS_INSTALL_SERVICE=0` | 不安装系统服务，改为便携安装 |
| `OXIDNS_START_SERVICE=0` | 安装服务但不立即启动 |

Linux / macOS 便携安装：

```bash
curl -fsSL https://oxidns.org/install.sh | env OXIDNS_INSTALL_SERVICE=0 sh
```

Windows 便携安装：

```powershell
$env:OXIDNS_INSTALL_SERVICE = "0"; irm https://oxidns.org/install.ps1 | iex
```

便携安装完成后可直接运行：

```bash
oxidns start -c ~/.oxidns/config.yaml -d ~/.oxidns
```

Windows：

```powershell
oxidns.exe start -c "$env:LOCALAPPDATA\OxiDNS\config.yaml" -d "$env:LOCALAPPDATA\OxiDNS"
```

默认配置包含 `:53` 监听。在 Linux / macOS 上，便携安装的普通用户通常不能绑定 53 端口；仅做本地验证时可先移除 `udp_server_53`，或使用默认的系统服务安装。

### 卸载

卸载脚本会移除二进制、`webui/` 和 PATH 入口，默认保留 `config.yaml`。

Linux / macOS：

```bash
curl -fsSL https://oxidns.org/uninstall.sh | sudo sh
```

Windows 管理员 PowerShell：

```powershell
irm https://oxidns.org/uninstall.ps1 | iex
```

如果安装时使用了 `sudo` 或自定义 `OXIDNS_INSTALL_DIR`，卸载时也请保持相同权限和目录变量。

彻底清理安装目录和配置：

```bash
curl -fsSL https://oxidns.org/uninstall.sh | sudo env OXIDNS_PURGE=1 sh
```

```powershell
$env:OXIDNS_PURGE = "1"; irm https://oxidns.org/uninstall.ps1 | iex
```

常用卸载变量：

| 变量 | 作用 |
| --- | --- |
| `OXIDNS_INSTALL_DIR` | 覆盖卸载目标目录，应与安装时一致 |
| `OXIDNS_BIN_DIR` | Linux / macOS 下覆盖命令链接目录 |
| `OXIDNS_NO_PATH=1` | 不移除命令链接或用户 PATH |
| `OXIDNS_UNINSTALL_SERVICE=1` | 强制卸载系统服务；默认在 root / 管理员环境中自动尝试 |
| `OXIDNS_PURGE=1` | 删除整个安装目录，包括配置 |

## 2. 从源码构建

适合需要调试、二次开发或暂时不依赖发行包的场景。

前提：

- 已安装稳定版 Rust 工具链
- 可用的 Cargo 构建环境

构建并运行：

```bash
git clone https://github.com/svenshi/oxidns.git
cd oxidns

cargo build --release
./target/release/oxidns check -c config.yaml
./target/release/oxidns start -c config.yaml
```

调试运行：

```bash
cargo run -- check -c config.yaml
cargo run -- start -c config.yaml -l debug
```

## 3. 使用 GitHub Releases 二进制包

OxiDNS 的 release 工作流会为多个平台生成独立二进制压缩包。压缩包内默认包含：

- `oxidns` 或 `oxidns.exe`
- `config.yaml`
- `LICENSE`
- `webui/`

发布页地址：

- [https://github.com/svenshi/oxidns/releases](https://github.com/svenshi/oxidns/releases)

### 支持的压缩包目标

非 Windows 平台使用 `.tar.gz`：

- `oxidns-x86_64-unknown-linux-gnu.tar.gz`
- `oxidns-x86_64-unknown-linux-musl.tar.gz`
- `oxidns-aarch64-unknown-linux-gnu.tar.gz`
- `oxidns-aarch64-unknown-linux-musl.tar.gz`
- `oxidns-i686-unknown-linux-musl.tar.gz`
- `oxidns-arm-unknown-linux-musleabihf.tar.gz`
- `oxidns-x86_64-apple-darwin.tar.gz`
- `oxidns-aarch64-apple-darwin.tar.gz`
- `oxidns-x86_64-unknown-freebsd.tar.gz`

Windows 平台使用 `.zip`：

- `oxidns-x86_64-pc-windows-msvc.zip`
- `oxidns-i686-pc-windows-msvc.zip`
- `oxidns-aarch64-pc-windows-msvc.zip`

### 不同系统如何选择 release 文件

无法确定应该下载哪个 asset 时，可按下面选择：

| 系统 / 环境 | 推荐 release 文件 | 说明 |
| --- | --- | --- |
| Linux x86_64 | `oxidns-x86_64-unknown-linux-musl.tar.gz` | 默认优先选这个，兼容性更稳 |
| Linux ARM64 | `oxidns-aarch64-unknown-linux-musl.tar.gz` | 默认优先选这个，兼容性更稳 |
| Debian / Ubuntu x86_64 服务安装 | `*_amd64.deb` | 适合 systemd 服务部署 |
| Debian / Ubuntu ARM64 服务安装 | `*_arm64.deb` | 适合 systemd 服务部署 |
| Alpine Linux x86_64 | `oxidns-x86_64-unknown-linux-musl.tar.gz` | Alpine 建议优先选 musl |
| Alpine Linux ARM64 | `oxidns-aarch64-unknown-linux-musl.tar.gz` | 静态链接更省心 |
| 明确是 glibc Linux 且需要动态链接版本 | `oxidns-x86_64-unknown-linux-gnu.tar.gz` / `oxidns-aarch64-unknown-linux-gnu.tar.gz` | 仅在明确环境匹配时选择 |
| 32 位 ARM Linux | `oxidns-arm-unknown-linux-musleabihf.tar.gz` | 适合部分树莓派和老 ARM 板子 |
| macOS Intel | `oxidns-x86_64-apple-darwin.tar.gz` | Intel Mac |
| macOS Apple Silicon | `oxidns-aarch64-apple-darwin.tar.gz` | M1 / M2 / M3 / M4 |
| Windows x64 | `oxidns-x86_64-pc-windows-msvc.zip` | 常见 PC |
| Windows 32-bit | `oxidns-i686-pc-windows-msvc.zip` | 仅在 32 位 Windows 上使用 |
| Windows ARM64 | `oxidns-aarch64-pc-windows-msvc.zip` | ARM Windows 设备 |
| FreeBSD x86_64 | `oxidns-x86_64-unknown-freebsd.tar.gz` | FreeBSD 主机 |

如果还不确定，可先查看本机信息：

```bash
uname -s
uname -m
```

常见输出和目标的对应关系：

- `Linux` + `x86_64`：默认选 `x86_64-unknown-linux-musl`，只有明确需要 glibc 动态链接版本时再选 `x86_64-unknown-linux-gnu`
- `Linux` + `aarch64`：默认选 `aarch64-unknown-linux-musl`，只有明确需要 glibc 动态链接版本时再选 `aarch64-unknown-linux-gnu`
- `Linux` + `armv7l`：选 `arm-unknown-linux-musleabihf`
- `Darwin` + `x86_64`：选 `x86_64-apple-darwin`
- `Darwin` + `arm64`：选 `aarch64-apple-darwin`

Windows 可在 PowerShell 中执行：

```powershell
[System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
```

输出为 `X64`、`Arm64` 或 `X86` 时，分别对应下载 `x86_64`、`aarch64`、`i686` 的 Windows release。

### Linux / macOS 安装示例

将下面的 `TAG` 替换为实际 release 标签，例如 `v0.1.0`。Linux 默认以 `x86_64-unknown-linux-musl` 为例：

```bash
curl -L -o oxidns.tar.gz \
  https://github.com/svenshi/oxidns/releases/download/TAG/oxidns-x86_64-unknown-linux-musl.tar.gz

mkdir -p oxidns
tar -xzf oxidns.tar.gz -C oxidns
cd oxidns

chmod +x oxidns
./oxidns check -c config.yaml
./oxidns start -c config.yaml
```

无法确认目标机的 glibc 版本，或运行在 Alpine Linux 上时，应优先选择 `*-linux-musl` 版本，不要默认使用 `gnu` 版本。

### Windows 安装示例

下载对应 `zip` 后解压，在 PowerShell 中运行：

```powershell
.\oxidns.exe start -c .\config.yaml
```

## 4. 使用 Debian 包安装

release 工作流当前会额外生成以下 Debian 包：

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

安装：

```bash
sudo dpkg -i oxidns_*_amd64.deb
```

或在 ARM64 Debian / Ubuntu 上：

```bash
sudo dpkg -i oxidns_*_arm64.deb
```

安装后默认文件位置：

- 二进制：`/usr/bin/oxidns`
- 配置：`/etc/oxidns/config.yaml`
- 运行期相对路径基准：`/var/lib/oxidns`
- WebUI：`/var/lib/oxidns/webui` 软链接到 `/usr/share/oxidns/webui`

项目也包含 systemd 打包配置，因此在 Debian 系发行版上适合直接作为系统服务部署。默认服务通过 `ExecStart ... -d /var/lib/oxidns` 明确指定运行期相对路径基准。

默认配置被修改后，建议先校验一次：

```bash
oxidns check -c /etc/oxidns/config.yaml -d /var/lib/oxidns
```

验证服务状态：

```bash
sudo systemctl status oxidns
```

如果服务尚未启动，可执行：

```bash
sudo systemctl enable --now oxidns
```

## 5. 使用 Docker 镜像

仓库提供 GHCR 镜像发布流程，镜像仓库地址为：

#### GitHub
- `ghcr.io/svenshi/oxidns`
#### Docker Hub
- `svenshi/oxidns`

当前 Docker 发布流程构建以下平台：

- `linux/amd64`
- `linux/arm64`

拉取并运行：

```bash
docker pull svenshi/oxidns:latest

docker run --rm \
  -p 53:53/udp \
  -p 53:53/tcp \
  -p 9199:9199/tcp \
  -v "$(pwd)/config.yaml:/etc/oxidns/config.yaml:ro" \
  svenshi/oxidns:latest
```

如果默认分支镜像已发布，也可以使用 `latest` 标签。

镜像默认启动命令等价于：

```bash
oxidns start -c /etc/oxidns/config.yaml -d /etc/oxidns
```

镜像内默认暴露：

- `53/udp`
- `53/tcp`
- `9199/tcp`

### Docker Compose 示例

使用 Compose 管理配置和端口映射时，可参考下面的 `docker-compose.yml`：

```yaml
services:
  oxidns:
    image: svenshi/oxidns:latest
    container_name: oxidns
    restart: unless-stopped
    ports:
      - "53:53/udp"
      - "53:53/tcp"
      - "9199:9199/tcp"
    volumes:
      - ./config.yaml:/etc/oxidns/config.yaml:ro
```

启动：

```bash
docker compose up -d
```

查看日志：

```bash
docker compose logs -f oxidns
```

## 6. 选择建议

- 想最快验证功能：使用一条命令安装最新 release，或手动下载对应 release 压缩包
- 想以系统服务长期运行：优先使用 Debian 包
- 想在容器平台部署：使用 GHCR Docker 或 Docker Hub 镜像
- 想参与开发或自行裁剪：从源码构建

## 7. 启动后下一步

启动完成后，建议继续阅读：

1. [配置总览](configuration.md)
2. [插件总览](plugin-reference/overview.md)
3. [常见策略场景](scenarios.md)
