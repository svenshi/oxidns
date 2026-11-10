---
title: Rules and Provider Syntax
---

This page collects the domain, IP, and data-source reference syntax shared by matchers, executors, and providers.

## Common Rule Syntax

### Domain Rules

These forms appear in plugins such as `qname`, `cname`, `domain_set`, `hosts`, and `redirect`:

- `full:example.com`
  - Exact match.
- `domain:example.com`
  - Suffix match.
- `keyword:cdn`
  - Substring match.
- `regexp:^api[0-9]+\\.example\\.com$`
  - Regular-expression match.
- `example.com`
  - Without a prefix, common domain-rule users such as `qname`, `cname`, and
    `domain_set` usually treat it as `domain:example.com`; `hosts` and
    `redirect` treat it as an exact `full:example.com` match.

For `hosts`, these domain rules apply to the default `format: oxidns`. With `format: standard`, use `<IP> <hostname> [aliases…]`; names match exactly without interpreting matcher prefixes, and addresses for each name are merged and deduplicated.

### IP Rules

These forms appear in `client_ip`, `resp_ip`, `ptr_ip`, `ip_set`, and related plugins:

- Single IP: `1.1.1.1`
- CIDR: `192.168.0.0/16`
- IPv6 CIDR: `2400:3200::/32`

### Provider References

Matchers and providers can reference providers through:

- `$tag`
  - References a defined provider with the required match capability.
  - Domain-oriented references can target `domain_set` or `geosite`.
  - IP-oriented references can target `ip_set` or `geoip`.
- `&/path/to/file`
  - Loads rules directly from a file.

Example:

```yaml
args:
  - "domain:example.com"
  - "$core_domains"
  - "&/etc/oxidns/domains.txt"
```
