---
title: Security Hardening and Vulnerability Reporting
---

OxiDNS commonly runs at the edge of a gateway, server, or home network. It handles sensitive DNS metadata and may also manage configuration, upgrades, and external network systems. This chapter defines a deployment-hardening baseline; the repository [`SECURITY.md`](https://github.com/svenshi/oxidns/blob/main/SECURITY.md) remains the authority for vulnerability disclosure.

## Keep the Management Plane on Trusted Networks

- Prefer binding the API to `127.0.0.1`, a management VLAN, or a VPN address.
- Remote access should use TLS plus Basic Auth, or a reverse proxy with strong authentication.
- Use mTLS for higher-sensitivity environments.
- Restrict sources with a firewall; never expose unauthenticated `/api/*` endpoints to the public Internet.
- WebUI static files are not protected by API Basic Auth, while every `/api/*` request still enforces API authentication.
- CORS is not access control. `allowed_origins` constrains browsers but does not block other HTTP clients.

Example for local-only management:

```yaml
api:
  http:
    listen: "127.0.0.1:9199"
    auth:
      type: basic
      username: ${ADMIN_USER}
      password: ${ADMIN_PASS}
```

For remote administration, prefer exposing this address through a VPN or reverse proxy instead of listening on every interface.

## Protect Configuration, Credentials, and Query Data

Treat these as sensitive:

- Passwords, tokens, proxy credentials, and RouterOS credentials in `config.yaml`.
- TLS private keys and client CAs.
- Query-recorder SQLite files, logs, and packet captures.
- Private names, client addresses, ECS information, and local provider rules.
- Upgrade caches, binary backups, and WebUI directories.

Recommended practices:

- Inject secrets from a controlled environment with `${VAR}` instead of committing credentials to Git.
- Restrict permissions on config, private keys, SQLite, and logs.
- Set an appropriate query-record retention period and avoid keeping unnecessary detail indefinitely.
- Redact internal names and client addresses as well as passwords before sharing config or logs.
- Give backups the same access controls and lifecycle as the source data.

## Run with Least Privilege

- A normal forwarding instance should not run as root indefinitely.
- Grant only the privileges needed for low ports, ipset/nftset, service installation, or route synchronization.
- Remove unused listeners, management features, and plugins from the configuration or custom bundle.
- Keep the OxiDNS working directory separate from writable directories owned by unrelated services.
- Avoid unnecessary privileged mode and host mounts in containers.

## High-Risk Plugins

| Plugin/capability | Main risk | Recommendation |
| --- | --- | --- |
| `script` | Executes external commands | Fix the command path and limit arguments and service-account privileges |
| `http_request` | Sends DNS-derived data externally | Restrict destinations, template fields, and logging |
| `download` | Downloads and replaces local files | Use HTTPS, controlled directories, and minimal write permissions |
| `upgrade` | Replaces the binary and WebUI | Protect the API, verify bundle selection, retain rollback backups |
| `ipset` / `nftset` | Changes host network state | Use dedicated sets and narrow capabilities |
| RouterOS plugins | Change external address lists or routes | Use a dedicated account, TLS, and an ownership namespace |
| `query_recorder` | Persists DNS query history | Restrict access, control retention, never publish the database |

Review failure policies, timeouts, concurrency limits, target paths, and cleanup behavior before enabling these capabilities.

## Upstreams and TLS

- Keep certificate verification enabled; use `insecure_skip_verify` only for bounded temporary diagnosis.
- Domain-based TLS/HTTPS upstreams require the correct SNI and certificate name. `dial_addr` changes the network destination without removing hostname verification.
- Bootstrap resolvers must be trusted and must not create a resolution loop.
- SOCKS5, remote resolvers, and webhooks are outbound trust boundaries and belong in the same network-policy review.
- Never log full proxy credentials, Authorization headers, or GitHub tokens publicly.

## Upgrade and Supply Chain

- Obtain artifacts from official GitHub Releases, official container repositories, or an auditable custom build.
- Automatic upgrades verify the release asset digest; manual downloads should also be checked against release information and hashes.
- Pin explicit versions in production instead of depending on a rolling `latest` tag for uncontrolled upgrades.
- Confirm platform and bundle before apply, then verify version, build capabilities, DNS, API, and WebUI.
- Back up configuration, SQLite, and provider data independently; a binary backup does not cover them.

## Report Vulnerabilities Privately

Do not open a public issue or post exploit details, private DNS data, or credentials in Telegram or Discussions.

Use either private channel:

- Email: `isvenshi@gmail.com`
- GitHub Security Advisory / private vulnerability reporting, when enabled for the repository

Include the affected version, platform, release asset or commit, a redacted minimal configuration or reproduction, impact, and whether the issue is remotely reachable. Test only systems you own or are explicitly authorized to assess.
