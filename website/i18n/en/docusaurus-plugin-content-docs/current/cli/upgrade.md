---
title: Upgrade Command
---


This page covers release checks, downloads, and application. Back up the binary, WebUI, configuration, and persistent data before a production upgrade, and prepare a rollback.

## `upgrade`

Checks, downloads, or applies OxiDNS upgrades from GitHub Releases.

Supported subcommands:

- `upgrade check`
- `upgrade download`
- `upgrade apply`

Common usage:

```bash
oxidns upgrade
oxidns upgrade --force
oxidns upgrade check
oxidns upgrade download --target latest
sudo oxidns upgrade apply
sudo oxidns upgrade apply --no-restart
```

Common arguments:

- `--target <TAG|latest>`
  - Release tag or `latest`.
  - Default: `latest`
- `--repository <OWNER/REPO>`
  - GitHub repository.
  - Default: `svenshi/oxidns`
- `--asset <NAME|auto>`
  - Release asset name. `auto` selects the archive for the current platform and build bundle.
  - Default: `auto`
- `-c, --config <PATH>`
  - Runtime configuration file used to read `api.http.webui.root` when `--webui-dir` is not set.
  - When omitted, `upgrade` first checks `config.yaml` in the current directory. On Linux package installs, it also uses `/etc/oxidns/config.yaml` when present.
- `-d, --working-dir <DIR>`
  - Base directory for runtime-relative paths, with the same semantics as `start -d/--working-dir`.
  - When omitted and the Linux package configuration is detected, `/var/lib/oxidns` is used; otherwise the current directory is used.
- `--bundle <auto|full|standard|minimal>`
  - Selects the release build bundle when `--asset auto` is used.
  - Default: `auto`, which follows the current binary's build bundle.
  - `full` uses the legacy asset name, for example `oxidns-x86_64-unknown-linux-musl.tar.gz`; `standard` / `minimal` use slim asset names such as `oxidns-standard-x86_64-unknown-linux-musl.tar.gz`.
- `--cache-dir <DIR>`
  - Directory for cached upgrade files.
  - Default: `./upgrade-cache`
- `--backup-dir <DIR>`
  - Directory for binary backups before `apply`.
  - Default: `./upgrade-backups`
- `--webui-dir <DIR>`
  - Directory where the WebUI static assets are installed during `apply`; relative paths are resolved against `-d/--working-dir`, and should stay aligned with `api.http.webui.root`.
  - When omitted, `upgrade` first infers it from `api.http.webui.root`; if no WebUI root is configured, it uses `./webui`.
- `--skip-webui`
  - For `apply`, skip the WebUI directory upgrade and replace only the binary.
- `--no-restart`
  - Skip restarting the service after a successful `apply`. By default the installed service is restarted automatically via the system service manager (systemd / launchd / Windows SCM).
- `--allow-prerelease`
  - Allows prerelease releases.
- `--force`
  - For `apply`, continue downloading, verifying, and replacing even when the selected release is not newer than the current version.
- `--timeout <DURATION>`
  - HTTP timeout such as `30s` or `2m`.
- `--socks5 <ADDR>`
  - Optional SOCKS5 proxy.
- `--insecure-skip-verify`
  - Disables TLS certificate verification.
- `--github-token <TOKEN>`
  - GitHub personal access token for API requests, used to raise the rate limit or access private repositories.

Behavior:

- `check` only queries the release and compares versions.
- `download` downloads the archive and verifies SHA256 with the GitHub release asset `digest` field.
- An explicit `--asset` always wins and skips `--bundle` inference.
- Omitting the subcommand defaults to `apply`.
- `apply` updates only when a newer version is available by default. `--force` forces the update.
- On Unix, `apply` unpacks the `.tar.gz`, backs up the current binary, and replaces it. On Windows, `apply` unpacks the `.zip`, backs up and replaces the binary, and also upgrades the WebUI directory.
- By default, after replacing the binary `apply` backs up and installs the archive's `webui/` directory into `--webui-dir`; `--skip-webui` skips it, and an archive without `webui/` is skipped without affecting the binary upgrade.
- In the default Debian package layout, `sudo oxidns upgrade apply` infers the WebUI directory from `/etc/oxidns/config.yaml` and `/var/lib/oxidns`; when `/var/lib/oxidns/webui` is a symlink, the real target directory is updated.
- After a successful `apply`, the service is restarted automatically via the system service manager. Pass `--no-restart` to skip the automatic restart.
- After a successful `apply`, the CLI asks whether to clean the cache and backup directories. The default answer is `Y`.
