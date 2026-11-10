---
title: 配置与数据工具
---


本页说明静态配置检查、编译能力检查和 V2Ray dat 数据导出。它们不会启动 DNS listener。

## `check`

静态检查配置文件是否有效，但不会真正启动 OxiDNS。

典型用法：

```bash
oxidns check -c config.yaml
oxidns check -c /etc/oxidns/config.yaml
oxidns check -c /etc/oxidns/config.yaml -d /var/lib/oxidns
oxidns check -c config.yaml --graph
```

参数说明：

- `-c, --config <PATH>`
  - 配置文件路径。
  - 默认值：`config.yaml`
- `-d, --working-dir <PATH>`
  - 校验前切换到指定工作目录。
  - 适合配置里使用相对路径时配合使用。
  - 建议与实际启动时的 `-d` 保持一致，避免校验和运行看到不同的相对路径。
- `--graph`
  - 校验成功后打印插件依赖图。

行为说明：

- 只做静态校验：
  - YAML 解析
  - 配置结构校验
  - 插件类型和依赖关系校验
- 不会初始化插件，不会绑定监听端口，也不会启动运行时。
- 校验成功时返回退出码 `0`，并输出简短成功信息。
- 传入 `--graph` 时，会额外按插件初始化顺序输出纯文本依赖图。
- 校验失败时返回非零退出码，并输出具体错误原因。

## `build-info`

输出当前 `oxidns` 二进制的编译期能力信息。

典型用法：

```bash
oxidns build-info
```

行为说明：

- 不读取配置文件，不启动运行时，也不会绑定端口。
- 输出格式为格式化 JSON。
- 输出内容包括：
  - `version`：当前包版本。
  - `bundle`：当前二进制的主编译组合包，可能为 `minimal`、`standard`、`full` 或 `custom`。
  - `enabled_bundles`：编译进当前二进制的 bundle feature。
  - `enabled_features`：公开的 Cargo feature 列表。
  - `supported_plugins`：当前二进制支持的 server、executor、matcher 和 provider 插件类型。
- 返回的编译能力对象与管理 API `GET /api/build` 响应中的 `build` 字段一致。

适用场景：

- 确认当前安装的是 `minimal`、`standard`、`full` 还是自定义构建。
- 排查某个协议、插件或 `upgrade` 子命令是否被编译进当前二进制。
- 在自定义构建、发布包验证或升级前后对比能力差异。

## `export-dat`

从 `geosite.dat` 或 `geoip.dat` 中导出指定 selector 到文本规则文件。

这些导出的文本文件可直接给 `domain_set.files` 或 `ip_set.files` 使用。

典型用法：

```bash
oxidns export-dat \
  --file ./rules/geosite.dat \
  --selector cn \
  --selector geolocation-\!cn \
  --out-dir ./rules/exported
```

额外生成并集文件：

```bash
oxidns export-dat \
  --file ./rules/geosite.dat \
  --kind geosite \
  --selector cn \
  --selector mastercard@cn \
  --out-dir ./rules/exported \
  --merged-file geosite_union.txt
```

导出 `geoip.dat`：

```bash
oxidns export-dat \
  --file ./rules/geoip.dat \
  --kind geoip \
  --selector cn \
  --out-dir ./rules/exported
```

不传 selector，直接导出整份 dat：

```bash
oxidns export-dat \
  --file ./rules/geosite.dat \
  --kind geosite \
  --out-dir ./rules/exported
```

指定原始格式导出：

```bash
oxidns export-dat \
  --file ./rules/geosite.dat \
  --kind geosite \
  --format original \
  --selector cn \
  --out-dir ./rules/exported
```

参数说明：

- `--file <PATH>`
  - `dat` 文件路径。
- `--kind <KIND>`
  - 指定 `dat` 类型。
  - 可选值：`auto` `geosite` `geoip`
  - 默认值：`auto`
- `--format <FORMAT>`
  - 指定文本导出格式。
  - 可选值：`oxidns` `original`
  - 默认值：`oxidns`
- `--selector <SELECTOR>`
  - 要导出的 selector。
  - 可重复传入多个，按输入顺序分别导出。
  - 不传时表示直接导出整份 dat。
- `--out-dir <DIR>`
  - 输出目录。
  - 不存在时会自动创建。
- `--merged-file <NAME>`
  - 可选。
  - 在输出目录中额外生成一个并集文件。
- `--overwrite`
  - 可选。
  - 允许覆盖已存在的目标文件。

行为说明：

- 默认按 selector 分别生成文件，例如 `cn.txt`、`geolocation-!cn.txt`。
- 不传 selector 时，会直接生成单个整表导出文件；默认文件名分别为 `geosite.txt` 或 `geoip.txt`。
- `geosite` 输出为 OxiDNS 域名规则格式，例如 `full:`、`domain:`、`keyword:`、`regexp:`。
- `oxidns` 格式会在导出文件头加入注释行，例如 `# selector: cn`；不传 selector 时为 `# selector: all`。
- `geosite` 在 `original` 格式下会保留原始类型语义，输出如 `plain:`、`regex:`、`root_domain:`、`full:`。
- `geosite` 的 `original` 格式会按 code 分组输出；如果域名带 attribute，会追加在域名后面，例如 `@cn`、`@ads=1`。
- `geoip` 输出为 IP / CIDR 纯文本规则。
- `geoip` 的 `oxidns` 格式同样会加入 selector 注释行。
- `geoip` 的 `original` 格式会按 code 分组输出，组头形式为 `[code]`。
- `geosite` selector 支持 `code@attribute`，例如 `mastercard@cn`。
- 任一 selector 没有匹配结果时，命令会直接失败，不会静默跳过。
