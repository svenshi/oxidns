import type { StandardUpstream } from "./types";

/**
 * Format one upstream for the connectivity-test API.
 *
 * Runtime Standard Mode generation is intentionally owned by the Rust
 * `/standard/plan` compiler. This browser helper does not generate OxiDNS
 * configuration and must not be used as a second compiler.
 */
export function upstreamAddress(upstream: StandardUpstream): string {
  const address = upstream.address.trim();
  if (upstream.protocol === "auto") return address;
  if (upstream.protocol === "udp") return withScheme(address, "udp://");
  if (upstream.protocol === "tcp") return withScheme(address, "tcp://");
  if (upstream.protocol === "dot") return withScheme(address, "tls://");
  if (upstream.protocol === "doq") return withScheme(address, "quic://");
  if (upstream.protocol === "doh" || upstream.protocol === "doh3") {
    const base = withScheme(address, "https://");
    if (base.includes("/", "https://".length)) return base;
    return `${base}${upstream.dohPath || "/dns-query"}`;
  }
  return address;
}

function withScheme(address: string, scheme: string): string {
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(address)
    ? address
    : `${scheme}${address}`;
}
