# OxiDNS v1.7.1

## 🚀 发布概览

- v1.7.1 是补丁版本，修复上游响应缓冲边界、DNS 压缩指针解析及容器单文件挂载的配置保存问题。

## ✨ 主要亮点

- DoH/H2/H3 上游响应体限制为 65,535 字节，限制 `Content-Length` 预分配并逐块校验实际长度，避免缺失或虚报长度头导致无界缓冲。
- DNS 名称解析以每次跳转 O(1) 的边界检查替代已访问偏移扫描，拒绝前向指针及跨标签/指针的循环，保留合法嵌套压缩后缀与既有跳转次数限制。
- Unix 配置保存遇到单文件挂载点的 `EBUSY` 时，校验文件身份后原地写入，保留 inode、权限与属主/属组；普通文件仍原子替换。补充 Docker 与 RouterOS Container 的配置目录挂载说明。

## ⚠️ 升级说明

- v1.7.0 YAML 配置可直接升级，没有新增、重命名或删除字段，默认值和 bundle 不变。超出上述大小限制的上游 DoH 响应、包含前向或循环压缩指针的 DNS 报文现在会被拒绝。
- **单文件挂载保存不具备原子性**：中断或 I/O 错误可能留下不完整配置，回退时会记录警告。建议可写挂载配置所在目录并保留备份；只读挂载仍无法通过 WebUI 保存。
- 升级前备份配置和持久化数据，并运行 `oxidns check -c <配置文件>`；更早版本仍须遵循对应版本的迁移说明。
- 根 crate 为 `1.7.1`，tag 为 `v1.7.1`；`oxidns-proto` 为 `0.1.8`、`oxidns-zoneparser` 为 `0.1.4`（更新协议依赖），`oxidns-macros` 保持 `0.1.1`、`oxidns-ripset` 保持 `0.1.3`。由发布 Action 按依赖顺序发布辅助 crate，再验证和发布根包。

## 📦 下载与校验

- 根据平台和 bundle 选择对应 archive，并使用 GitHub Release asset 的 SHA256 digest 校验下载文件。

## ✅ 验证

- `just check`
- `just check-matrix`
- 协议与 zoneparser：`cargo test --locked -p oxidns-proto -p oxidns-zoneparser`。
- 双语文档：在 `website/` 运行 `npm run build`。
- Telegram 发布消息：`python3 -m unittest discover -s .github/scripts -p '*_test.py'`。
- full/minimal：`cargo build --locked --no-default-features --features full` / `minimal`，以及 `oxidns --version`、`oxidns build-info` 和对应的 `oxidns check -c config.yaml` / `oxidns check -c config.minimal.yaml`。
- crates.io workspace dry-run：`cargo publish --locked --dry-run --allow-dirty --workspace`（包含打包后编译验证，未上传）。
