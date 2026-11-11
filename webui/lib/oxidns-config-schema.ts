/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

export const OXIDNS_CONFIG_TOP_LEVEL_KEYS = [
  "include",
  "runtime",
  "api",
  "log",
  "network",
  "plugins",
] as const;

export const OXIDNS_LOG_LEVELS = [
  "off",
  "trace",
  "debug",
  "info",
  "warn",
  "error",
] as const;

export const OXIDNS_LOG_ROTATION_TYPES = [
  "never",
  "minutely",
  "hourly",
  "daily",
  "weekly",
] as const;

export const OXIDNS_NAMESERVER_SCHEME_EXAMPLES = [
  "udp://1.1.1.1:53",
  "tcp://1.1.1.1:53",
  "tcp+pipeline://1.1.1.1:53",
  "tls://dns.google:853",
  "tls+pipeline://dns.google:853",
  "https://cloudflare-dns.com/dns-query",
  "doh://cloudflare-dns.com/dns-query",
  "h3://cloudflare-dns.com/dns-query",
  "quic://94.140.14.14:853",
  "doq://94.140.14.14:853",
] as const;

export interface OxiDnsConfigValueSuggestion {
  label: string;
  apply?: string;
  detail?: string;
  type?: "constant" | "enum" | "text";
}

const topLevelOrder = [...OXIDNS_CONFIG_TOP_LEVEL_KEYS];

const orderByPath = new Map<string, readonly string[]>([
  ["", topLevelOrder],
  ["runtime", ["worker_threads"]],
  ["api", ["http"]],
  ["api.http", ["listen", "ssl", "auth", "cors", "webui"]],
  ["api.http.ssl", ["cert", "key", "client_ca", "require_client_cert"]],
  ["api.http.auth", ["type", "username", "password"]],
  ["api.http.cors", ["allowed_origins"]],
  ["api.http.webui", ["root", "index"]],
  ["log", ["level", "file", "rotation"]],
  ["log.rotation", ["type", "max_files"]],
  ["network", ["outbound"]],
  ["network.outbound", ["default", "profiles"]],
  ["network.outbound.profiles.*", ["resolver", "proxy"]],
  [
    "network.outbound.profiles.*.resolver",
    ["nameservers", "ip_version", "timeout", "proxy"],
  ],
  ["network.outbound.profiles.*.resolver.nameservers.*", ["addr", "dial_addr"]],
  ["network.outbound.profiles.*.proxy", ["socks5"]],
  ["plugins.*", ["tag", "type", "args"]],
]);

export function getOxiDnsConfigSubKeys(path: string[]): string[] | null {
  const normalized = normalizeConfigPath(path);
  if (normalized.length === 0) return [...OXIDNS_CONFIG_TOP_LEVEL_KEYS];

  const exact = orderByPath.get(normalized.join("."));
  if (exact) return [...exact];

  const [p0, p1, p2, , p4, p5] = normalized;
  if (p0 === "plugins") {
    if (normalized.includes("args")) return null;
    return ["tag", "type", "args"];
  }
  if (p0 === "network" && p1 === "outbound" && p2 === "profiles") {
    if (normalized.length === 3) return null;
    if (normalized.length === 4) return ["resolver", "proxy"];
    if (p4 === "resolver") {
      if (p5 === "nameservers") return ["addr", "dial_addr"];
      if (normalized.includes("nameservers")) return ["addr", "dial_addr"];
      return ["nameservers", "ip_version", "timeout", "proxy"];
    }
    if (p4 === "proxy") return ["socks5"];
  }

  return null;
}

export function getOxiDnsConfigValueSuggestions(
  path: string[],
  valueKey: string | null,
): OxiDnsConfigValueSuggestion[] {
  if (!valueKey) return [];
  const normalized = normalizeConfigPath(path);
  const joined = normalized.join(".");

  if (valueKey === "level" && joined.startsWith("log")) {
    return OXIDNS_LOG_LEVELS.map((level) => ({
      label: level,
      type: "enum",
    }));
  }
  if (valueKey === "type" && joined.startsWith("log.rotation")) {
    return OXIDNS_LOG_ROTATION_TYPES.map((type) => ({
      label: type,
      type: "enum",
    }));
  }
  if (valueKey === "type" && joined.startsWith("api.http.auth")) {
    return [{ label: "basic", type: "enum" }];
  }
  if (valueKey === "require_client_cert") {
    return booleanSuggestions();
  }
  if (valueKey === "resolver" && joined.includes("network.outbound.profiles")) {
    return [{ label: "system", type: "enum" }];
  }
  if (valueKey === "ip_version") {
    return [
      { label: "4", type: "enum" },
      { label: "6", type: "enum" },
    ];
  }
  if (valueKey === "proxy") {
    if (joined.includes(".resolver")) {
      return [
        { label: "none", type: "enum" },
        { label: "profile", type: "enum" },
      ];
    }
    if (joined.includes("network.outbound.profiles")) {
      return [
        { label: "none", type: "enum" },
        { label: "direct", type: "enum" },
      ];
    }
  }
  if (valueKey === "addr" && joined.includes(".nameservers")) {
    return OXIDNS_NAMESERVER_SCHEME_EXAMPLES.map((example) => ({
      label: example,
      apply: example,
      type: "text",
    }));
  }

  return [];
}

export function sortOxiDnsConfigForSerialize(value: unknown): unknown {
  return sortKnownConfigValue(value, []);
}

export function extractOutboundProfileNames(config: unknown): string[] {
  if (!isPlainRecord(config)) return [];
  const network = asRecord(config.network);
  const outbound = asRecord(network.outbound);
  const profiles = asRecord(outbound.profiles);
  return Object.keys(profiles).filter((name) => name.trim().length > 0);
}

function booleanSuggestions(): OxiDnsConfigValueSuggestion[] {
  return [
    { label: "true", type: "constant" },
    { label: "false", type: "constant" },
  ];
}

function sortKnownConfigValue(value: unknown, path: string[]): unknown {
  if (isPluginArgsPath(path)) return value;
  if (Array.isArray(value)) {
    return value.map((entry) => sortKnownConfigValue(entry, [...path, "*"]));
  }
  if (!isPlainRecord(value)) return value;

  const order = orderForPath(path);
  const sortedEntries = sortEntriesByKnownOrder(Object.entries(value), order);
  return Object.fromEntries(
    sortedEntries.map(([key, entry]) => [
      key,
      sortKnownConfigValue(entry, [...path, key]),
    ]),
  );
}

function orderForPath(path: string[]): readonly string[] | undefined {
  return orderByPath.get(normalizeConfigPath(path).join("."));
}

function sortEntriesByKnownOrder(
  entries: [string, unknown][],
  order: readonly string[] | undefined,
) {
  if (!order) return entries;
  const indexByKey = new Map(order.map((key, index) => [key, index]));
  return [...entries].sort(([left], [right]) => {
    const leftIndex = indexByKey.get(left);
    const rightIndex = indexByKey.get(right);
    if (leftIndex === undefined && rightIndex === undefined) return 0;
    if (leftIndex === undefined) return 1;
    if (rightIndex === undefined) return -1;
    return leftIndex - rightIndex;
  });
}

function normalizeConfigPath(path: string[]): string[] {
  if (
    path[0] === "network" &&
    path[1] === "outbound" &&
    path[2] === "profiles"
  ) {
    return path.map((part, index) => (index === 3 ? "*" : part));
  }
  return path;
}

function isPluginArgsPath(path: string[]) {
  return path[0] === "plugins" && path[2] === "args";
}

function asRecord(value: unknown): Record<string, unknown> {
  return isPlainRecord(value) ? value : {};
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}
