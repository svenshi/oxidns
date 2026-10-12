# Upstream synchronization

- Repository: <https://github.com/erikoest/zoneparser>
- Reviewed branch: `main` (`refs/heads/main`, upstream default branch)
- Reviewed commit: `80909d7f4272564b9529b335ae798f65efa10e6d`
- Commit link: <https://github.com/erikoest/zoneparser/commit/80909d7f4272564b9529b335ae798f65efa10e6d>
- Reviewed on: 2026-10-12

This is the latest upstream revision compared against this crate, not a claim
that the two source trees or public APIs are identical. The earlier import did
not record an upstream revision, so this review checked the current upstream
library and fixtures, including the changes in the following commits.

## Review decisions

| Upstream commit | Scope | Local outcome |
| --- | --- | --- |
| `3e7941d251d12e55bc9a58d0f1f6af2aa21ea334` | Decimal byte escapes, quoted strings, parser errors, relative origins, and bracket checks | Text RDATA now rejects incomplete decimal escapes and values above 255 without overflow. Quoted strings spanning physical lines require parentheses. Nesting is bounded at 1000 levels. Existing structured I/O/header errors, unmatched-bracket checks, and relative-origin resolution are retained. New and expanded upstream fixtures cover decimal bytes, unterminated quotes, multiline text, and mixed-case directives. |
| `b1264f7bdf6d323ea2b44b2cea7f72fd410c8b0b` | Numeric `TYPE####` formatting | Applied to parser diagnostics and generated record headers, fixing `$GENERATE` for unknown record types and preserving `TYPE0` instead of the internal `ZERO` name. The shared record model stays in `oxidns-proto`; upstream's `RRType`/`Record` implementations are not copied. |
| `80909d7f4272564b9529b335ae798f65efa10e6d` | Example CLI tools, CLI argument checks, and tool dependencies | No library parser changes. OxiDNS does not vendor these tools, so their code and `base64`/`chrono` dependencies are not imported. |

## Retained OxiDNS behavior

- `ParseOptions`, collecting APIs, and streaming visitors replace the upstream
  iterator API. Output is `oxidns_proto::Record`, with typed RDATA and canonical
  domain names rather than upstream's strings and anonymous data fields.
- `$INCLUDE`, `$GENERATE`, TTL units, hash comments, and RFC3597 wire decoding
  remain local extensions. Includes retain their configured depth bound.
- Text RDATA contains the actual escaped bytes, including non-UTF-8 bytes,
  rather than upstream's printable byte representation. Newlines and spaces
  inside parenthesized quoted strings are preserved.
- The fixtures retain local valid IPv6 presentation (`1:2:3:4::`), MX preference
  values, and RFC3597 payloads for anonymous types. Upstream does not validate
  typed RDATA and some of its fixtures are unsuitable for this crate unchanged.

Future reviews should compare upstream changes after the recorded commit,
evaluate them against these local contracts, and update the branch, full hash,
date, and review decisions together.
