# OxiDNS v1.6.0

## 🚀 发布概览

- v1.6.0 更新 `query_recorder` 历史存储和 Windows 服务恢复机制，并改进计划任务、手动下载、DNS 网络传输与 WebUI 配置编辑。
- **升级前必读：查询历史从 v2 重新开始；Windows 已安装服务需要重新安装。**

## ✨ 主要亮点

- `query_recorder` v2 复用重复的执行路径与问题列表，在后台无损压缩适用的响应快照；现有配置、查询 API、过滤、统计和 SSE 保持兼容。
- Windows 服务通过 SCM 恢复动作处理应用重启，修复重启信号丢失；新恢复设置在服务安装时写入。
- 计划任务支持手动运行与结果追踪；下载执行器增加手动触发和状态控制。
- 改进 UDP 回复源地址与 Windows UDP 套接字行为，修复客户端断开时的在途 DNS 工作处理；支持 ipset 协议 6 并保留 nftset 查询错误。
- WebUI 改进插件配置编辑、YAML 格式保留和配置补丁确认。

## ⚠️ 升级说明

- **`query_recorder` 数据库文件需按需手动清理**：旧 v1 历史不会迁移、显示或自动删除，也不会被“清空历史”或保留期清理移除。若不需要保留任何历史，先停止 OxiDNS 并备份；仅在确认 `query_recorder.path` 对应文件没有被其他 recorder 共用后，手动删除 SQLite 文件及同名 `-wal`、`-shm` 文件，再启动服务。共用文件不能整文件删除，应备份后只清理目标 recorder 的旧 v1 表。相对路径以运行工作目录为基准。
- **Windows 用户需重新安装服务**：在管理员终端执行 `oxidns.exe service stop`、`oxidns.exe service uninstall`，替换为 v1.6.0 二进制，再用原路径执行 `oxidns.exe service install -d <绝对工作目录> -c <配置文件>` 和 `oxidns.exe service start`。仅替换二进制或重启旧服务不会写入新的 SCM 恢复设置。
- 现有 YAML 配置可直接升级；替换二进制前建议运行 `oxidns check -c <配置文件>`。根 crate 为 `1.6.0`，`oxidns-proto` 为 `0.1.6`，`oxidns-ripset` 为 `0.1.3`；tag 为 `v1.6.0`。

## 📦 下载与校验

- 根据平台和 bundle 选择对应 archive，并使用 GitHub Release asset 的 digest 校验下载文件。
