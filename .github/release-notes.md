# OxiDNS v1.6.1

## 🚀 发布概览

- v1.6.1 是补丁版本，修复上游连接关闭后的资源释放与 IPv6 字面量解析，并改进连接池诊断。

## ✨ 主要亮点

- TCP/UDP 连接关闭后及时结束后台收发任务、取消等待中的查询，避免已关闭连接持续占用内存与套接字；TCP 写入失败或阻塞时也能退出。
- 正确识别上游 URL 中带方括号的 IPv6 地址，直接连接，不再误走域名 bootstrap 解析；上游探测同步修正。
- 连接池与关闭日志增加上游标识、主机、端口和传输信息；正常并发转发中的查询取消改为 debug 日志。
- 更新 Rust 依赖。

## ⚠️ 升级说明

- v1.6.0 YAML 配置可直接升级，配置字段、默认值与 bundle 不变。替换二进制前建议运行 `oxidns check -c <配置文件>`。
- 从 v1.6.0 升级无需迁移查询历史或重新安装 Windows 服务；从更早版本升级仍需遵循 v1.6.0 的迁移说明。
- 根 crate 为 `1.6.1`，辅助 crate 版本保持不变；tag 为 `v1.6.1`。

## 📦 下载与校验

- 根据平台和 bundle 选择对应 archive，并使用 GitHub Release asset 的 SHA256 digest 校验下载文件。

## ✅ 验证

- `just check`
- `just check-matrix`
- 文档：`npm run build`
- Telegram 发布消息：`python3 -m unittest discover -s .github/scripts -p '*_test.py'`
- crates.io dry-run：`cargo publish --locked --dry-run --allow-dirty`
- full/minimal：`oxidns build-info`、`oxidns --version`，以及对应的 `oxidns check -c config.yaml` / `oxidns check -c config.minimal.yaml`
