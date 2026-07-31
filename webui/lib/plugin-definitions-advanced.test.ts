import { describe, expect, it } from "vitest";

import {
  pluginKindDefinitions,
  type ConfigField,
  type ConfigFieldChild,
} from "@/lib/plugin-definitions";

const EXPECTED_ADVANCED_FIELDS: Record<string, string[]> = {
  "server/tcp_server": ["idle_timeout"],
  "server/http_server": ["entries[].json_api", "idle_timeout", "enable_http3"],
  "server/quic_server": ["idle_timeout"],
  "executor/forward": [
    "concurrent",
    "response_selection",
    "upstreams[].outbound",
    "upstreams[].dial_addr",
    "upstreams[].port",
    "upstreams[].bootstrap",
    "upstreams[].bootstrap_version",
    "upstreams[].socks5",
    "upstreams[].idle_timeout",
    "upstreams[].max_conns",
    "upstreams[].min_conns",
    "upstreams[].timeout",
    "upstreams[].enable_pipeline",
    "upstreams[].enable_http3",
    "upstreams[].so_mark",
    "upstreams[].bind_to_device",
  ],
  "executor/cache": [
    "dump_file",
    "dump_interval",
    "max_negative_ttl",
    "negative_ttl_without_soa",
    "max_positive_ttl",
    "min_positive_ttl",
    "ecs_in_key",
  ],
  "executor/fallback": ["always_standby"],
  "executor/response": ["authoritative", "authentic_data"],
  "executor/ecs_handler": ["mask4", "mask6"],
  "executor/ip_selector": [
    "outbound",
    "socks5",
    "probe_stagger",
    "probe_timeout",
    "max_wait",
    "dnssec_policy",
    "max_parallel_probes",
    "cache",
  ],
  "executor/prefer_ipv4": ["cache_ttl"],
  "executor/prefer_ipv6": ["cache_ttl"],
  "executor/reverse_lookup": ["size", "ttl"],
  "executor/learn_domain": ["async", "timeout"],
  "executor/query_recorder": [
    "queue_size",
    "batch_size",
    "flush_interval_ms",
    "memory_tail",
    "cleanup_interval_hours",
    "reader_concurrency",
    "include_marks",
    "exclude_marks",
  ],
  "executor/http_request": [
    "async",
    "timeout",
    "outbound",
    "socks5",
    "max_redirects",
    "queue_size",
  ],
  "executor/script": ["timeout", "max_output_bytes"],
  "executor/ipset": ["mask4", "mask6"],
  "executor/nftset": [
    "ipv4.mask",
    "ipv6.mask",
    "table_family4",
    "table_name4",
    "set_name4",
    "mask4",
    "table_family6",
    "table_name6",
    "set_name6",
    "mask6",
  ],
  "executor/ros_route": [
    "tls",
    "connect_timeout",
    "send_timeout",
    "receive_timeout",
    "async",
    "wait_timeout",
    "queue_capacity",
    "distance",
    "comment_prefix",
    "persistent",
    "min_ttl",
    "max_ttl",
    "fixed_ttl",
    "conntrack_guard",
    "cleanup_on_shutdown",
  ],
  "executor/ros_address_list": [
    "tls",
    "connect_timeout",
    "send_timeout",
    "receive_timeout",
    "async",
    "wait_timeout",
    "queue_capacity",
    "comment_prefix",
    "persistent",
    "min_ttl",
    "max_ttl",
    "fixed_ttl",
    "cleanup_on_shutdown",
  ],
  "executor/upgrade": [
    "github_token",
    "cache_dir",
    "backup_dir",
    "timeout",
    "outbound",
    "socks5",
  ],
  "executor/download": ["timeout", "outbound", "socks5"],
  "executor/cron": ["timezone"],
  "matcher/time": ["timezone"],
  "matcher/rate_limiter": ["mask4", "mask6"],
  "provider/dynamic_domain_set": [
    "queue_size",
    "batch_size",
    "flush_interval_ms",
  ],
};

function collectAdvancedPaths(fields: ConfigField[], parent = ""): string[] {
  return fields.flatMap((field) => {
    const path = parent ? `${parent}.${field.key}` : field.key;
    const paths = field.advanced ? [path] : [];

    if (field.fields) {
      paths.push(...collectAdvancedPaths(field.fields, path));
    }
    if (field.item?.type === "object") {
      paths.push(...collectAdvancedPaths(field.item.fields, `${path}[]`));
    }
    for (const item of field.itemOptions ?? []) {
      if (item.type === "object") {
        paths.push(...collectAdvancedPaths(item.fields, `${path}[]`));
      }
    }

    return paths;
  });
}

function collectRequiredAdvancedPaths(
  fields: ConfigField[],
  parent = "",
): string[] {
  return fields.flatMap((field) => {
    const path = parent ? `${parent}.${field.key}` : field.key;
    const paths = field.required && field.advanced ? [path] : [];
    const children: ConfigFieldChild[] = [
      ...(field.item ? [field.item] : []),
      ...(field.itemOptions ?? []),
    ];

    if (field.fields) {
      paths.push(...collectRequiredAdvancedPaths(field.fields, path));
    }
    for (const child of children) {
      if (child.type === "object") {
        paths.push(...collectRequiredAdvancedPaths(child.fields, `${path}[]`));
      }
    }

    return paths;
  });
}

describe("plugin advanced field classification", () => {
  it("covers the complete plugin registry with the reviewed field paths", () => {
    expect(pluginKindDefinitions).toHaveLength(65);

    const actual = Object.fromEntries(
      pluginKindDefinitions
        .map((definition) => [
          `${definition.type}/${definition.kind}`,
          collectAdvancedPaths(definition.configSchema),
        ])
        .filter(([, paths]) => paths.length > 0),
    );

    expect(actual).toEqual(EXPECTED_ADVANCED_FIELDS);
  });

  it("keeps required configuration visible", () => {
    const hiddenRequiredFields = pluginKindDefinitions.flatMap((definition) =>
      collectRequiredAdvancedPaths(definition.configSchema).map(
        (path) => `${definition.type}/${definition.kind}.${path}`,
      ),
    );

    expect(hiddenRequiredFields).toEqual([]);
  });
});
