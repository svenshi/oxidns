---
title: Configuration and Data Tools
---


This page covers static configuration checks, compiled-capability inspection, and V2Ray dat export. These commands do not start DNS listeners.

## `check`

Statically validates a configuration file without starting OxiDNS.

Typical usage:

```bash
oxidns check -c config.yaml
oxidns check -c /etc/oxidns/config.yaml
oxidns check -c /etc/oxidns/config.yaml -d /var/lib/oxidns
oxidns check -c config.yaml --graph
```

Arguments:

- `-c, --config <PATH>`
  - Path to the configuration file.
  - Default: `config.yaml`
- `-d, --working-dir <PATH>`
  - Change to the specified working directory before validation.
  - Useful when the config relies on relative paths.
  - Keep it the same as the runtime `-d` value so validation and startup see the same relative paths.
- `--graph`
  - Print the plugin dependency graph after validation succeeds.

Behavior:

- Performs static validation only:
  - YAML parsing
  - schema-level config validation
  - plugin type and dependency validation
- Does not initialize plugins, bind listeners, or start the runtime.
- On success, exits with code `0` and prints a short success line.
- With `--graph`, it also prints a plain-text dependency graph in plugin initialization order.
- On failure, exits non-zero and prints the validation error.

## `build-info`

Prints the compile-time capabilities of the current `oxidns` binary.

Typical usage:

```bash
oxidns build-info
```

Behavior:

- Does not read a configuration file, start the runtime, or bind any ports.
- Prints formatted JSON.
- The output includes:
  - `version`: current package version.
  - `bundle`: primary build bundle for this binary: `minimal`, `standard`, `full`, or `custom`.
  - `enabled_bundles`: bundle features compiled into the binary.
  - `enabled_features`: public Cargo features compiled into the binary.
  - `supported_plugins`: server, executor, matcher, and provider plugin types supported by this binary.
- The returned capability object matches the `build` field returned by the management API `GET /api/build`.

Common use cases:

- Confirm whether the installed binary is `minimal`, `standard`, `full`, or a custom build.
- Check whether a protocol, plugin, or the `upgrade` subcommand is compiled into the current binary.
- Compare capabilities before and after custom builds, package validation, or upgrades.

## `export-dat`

Exports selected rules from `geosite.dat` or `geoip.dat` into text rule files.

These exported files can be referenced directly from `domain_set.files` or `ip_set.files`.

Typical usage:

```bash
oxidns export-dat \
  --file ./rules/geosite.dat \
  --selector cn \
  --selector geolocation-\!cn \
  --out-dir ./rules/exported
```

Generate an additional merged union file:

```bash
oxidns export-dat \
  --file ./rules/geosite.dat \
  --kind geosite \
  --selector cn \
  --selector mastercard@cn \
  --out-dir ./rules/exported \
  --merged-file geosite_union.txt
```

Export from `geoip.dat`:

```bash
oxidns export-dat \
  --file ./rules/geoip.dat \
  --kind geoip \
  --selector cn \
  --out-dir ./rules/exported
```

Export the entire dat file without selectors:

```bash
oxidns export-dat \
  --file ./rules/geosite.dat \
  --kind geosite \
  --out-dir ./rules/exported
```

Export using the original text format:

```bash
oxidns export-dat \
  --file ./rules/geosite.dat \
  --kind geosite \
  --format original \
  --selector cn \
  --out-dir ./rules/exported
```

Arguments:

- `--file <PATH>`
  - Path to the source `dat` file.
- `--kind <KIND>`
  - Explicit `dat` kind.
  - Values: `auto` `geosite` `geoip`
  - Default: `auto`
- `--format <FORMAT>`
  - Output text format.
  - Values: `oxidns` `original`
  - Default: `oxidns`
- `--selector <SELECTOR>`
  - Selector to export.
  - Repeat the flag to export multiple selectors.
  - Omit it to export the entire dat file.
- `--out-dir <DIR>`
  - Output directory.
  - It is created automatically when missing.
- `--merged-file <NAME>`
  - Optional.
  - Writes one extra merged union file inside the output directory.
- `--overwrite`
  - Optional.
  - Allows replacing existing output files.

Behavior:

- By default, OxiDNS writes one file per selector, for example `cn.txt` or `geolocation-!cn.txt`.
- When no selector is provided, OxiDNS writes one full-export file named `geosite.txt` or `geoip.txt` by default.
- `geosite` exports OxiDNS domain rule expressions such as `full:`, `domain:`, `keyword:`, and `regexp:`.
- In `oxidns` format, exported files add a header comment such as `# selector: cn`; when no selector is provided, the header becomes `# selector: all`.
- In `original` format, `geosite` preserves the source type names and writes values such as `plain:`, `regex:`, `root_domain:`, and `full:`.
- In `original` format, `geosite` output is grouped by code, and domain attributes are appended after the domain text, for example `@cn` or `@ads=1`.
- `geoip` exports plain IP / CIDR lines.
- In `oxidns` format, `geoip` exports also include selector header comments.
- In `original` format, `geoip` output is grouped by code with section headers like `[code]`.
- `geosite` selectors support `code@attribute`, for example `mastercard@cn`.
- If any selector matches no rules, the command fails instead of silently skipping it.
