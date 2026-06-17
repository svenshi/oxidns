import { isPluginKindSupported } from "../build-capabilities";
import type { BuildInfo } from "../oxidns-api";
import type {
  OxiDnsConfig,
  OxiDnsPluginConfig as PluginConfig,
} from "../oxidns-config";
import type { PluginType } from "../types";
import type {
  StandardGenerationResult,
  StandardGenerationSummary,
  StandardModeSettings,
  StandardResolutionPath,
  StandardTagMap,
  StandardUpstream,
  StandardUpstreamGroup,
} from "./types";

export const STANDARD_PLUGIN_TAGS = [
  "standard_metrics",
  "standard_recorder",
  "standard_cache",
  "standard_ad_rules",
  "standard_blocked",
  "standard_forward_default",
  "standard_path_default",
  "standard_main_sequence",
  "standard_udp",
  "standard_tcp",
] as const;

const STANDARD_TAG_SET = new Set<string>(STANDARD_PLUGIN_TAGS);

function plugin(type: PluginType, tag: string, args?: unknown): PluginConfig {
  return { type, tag, args: args ?? {} };
}

function supports(
  buildInfo: BuildInfo | null,
  type: PluginType,
  kind: string,
): boolean {
  return isPluginKindSupported(buildInfo, type, kind);
}

function standardTag(prefix: string, id: string): string {
  const safe = id
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "_")
    .replace(/^_+|_+$/g, "");
  return `standard_${prefix}_${safe || "default"}`;
}

function enabledUpstreams(upstreams: StandardUpstream[]) {
  return upstreams
    .filter((item) => item.enabled && item.address.trim())
    .map((item) => ({
      tag: item.id,
      addr: upstreamAddress(item),
      ...(item.bootstrap ? { bootstrap: item.bootstrap } : {}),
      ...(item.dialAddress ? { dial_addr: item.dialAddress } : {}),
      ...(item.tlsVerify === false ? { insecure_skip_verify: true } : {}),
      ...(item.protocol === "doh3" || item.enableHttp3
        ? { enable_http3: true }
        : {}),
    }));
}

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
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(address) ? address : `${scheme}${address}`;
}

function concurrentCount(
  upstreams: ReturnType<typeof enabledUpstreams>,
  strategy: StandardUpstreamGroup["strategy"],
): number {
  if (strategy === "sequential") return 1;
  return Math.max(1, Math.min(3, upstreams.length));
}

function blockMode(settings: StandardModeSettings) {
  if (settings.filtering.blockResponse === "nxdomain") return "nxdomain";
  if (settings.filtering.blockResponse === "refused") return "refused";
  return "null";
}

export function isStandardManagedPlugin(tag: string): boolean {
  return (
    STANDARD_TAG_SET.has(tag as (typeof STANDARD_PLUGIN_TAGS)[number]) ||
    tag.startsWith("standard_")
  );
}

export function generateStandardConfig(
  settings: StandardModeSettings,
  buildInfo: BuildInfo | null,
  baseConfig?: OxiDnsConfig | null,
): StandardGenerationResult {
  const generated: PluginConfig[] = [];
  const skippedCapabilities: string[] = [];
  const tagMap: StandardTagMap = {
    system: [],
    upstreamGroups: {},
    paths: {},
    routingRules: {},
    exceptionRules: {},
  };

  const pushIfSupported = (
    capability: string,
    type: PluginType,
    kind: string,
    tag: string,
    args?: unknown,
  ) => {
    if (!supports(buildInfo, type, kind)) {
      skippedCapabilities.push(capability);
      return false;
    }
    generated.push(plugin(type, tag, args));
    return true;
  };

  if (
    pushIfSupported(
      "metrics",
      "executor",
      "metrics_collector",
      "standard_metrics",
      {},
    )
  ) {
    tagMap.system.push("standard_metrics");
  }

  if (
    settings.queryLog.enabled &&
    pushIfSupported(
      "query_recorder",
      "executor",
      "query_recorder",
      "standard_recorder",
      {
        path: "./data/standard-query-recorder.sqlite",
        queue_size: 8192,
        batch_size: 256,
        flush_interval_ms: 200,
        memory_tail: 1024,
        retention_days: Math.max(1, settings.queryLog.retentionDays),
        cleanup_interval_hours: 1,
      },
    )
  ) {
    tagMap.queryLog = "standard_recorder";
  }

  if (
    settings.cache.enabled &&
    pushIfSupported("cache", "executor", "cache", "standard_cache", {
      size: settings.cache.size,
      min_ttl: settings.cache.minTtl,
      max_ttl: settings.cache.maxTtl,
      negative_ttl: settings.cache.negativeTtl,
      short_circuit: true,
    })
  ) {
    tagMap.cache = "standard_cache";
  }

  const hasFilteringRules =
    settings.filtering.enabled && settings.filtering.blockRules.length > 0;
  if (
    hasFilteringRules &&
    pushIfSupported(
      "ad_rules",
      "provider",
      "adguard_rule",
      "standard_ad_rules",
      {
        files: [],
        rules: settings.filtering.blockRules,
      },
    )
  ) {
    tagMap.filtering = ["standard_ad_rules"];
  }
  if (
    hasFilteringRules &&
    pushIfSupported(
      "black_hole",
      "executor",
      "black_hole",
      "standard_blocked",
      {
        mode: blockMode(settings),
        short_circuit: true,
      },
    )
  ) {
    tagMap.filtering = [...(tagMap.filtering ?? []), "standard_blocked"];
  }

  const groupsById = new Map(settings.upstreamGroups.map((group) => [group.id, group]));
  for (const group of settings.upstreamGroups) {
    const upstreams = enabledUpstreams(group.upstreams);
    if (upstreams.length === 0) continue;
    const tag = standardTag("forward", group.id);
    if (
      pushIfSupported("forward", "executor", "forward", tag, {
        upstreams,
        concurrent: concurrentCount(upstreams, group.strategy),
      })
    ) {
      tagMap.upstreamGroups[group.id] = tag;
    }
  }

  for (const path of settings.paths) {
    const group = groupsById.get(path.upstreamGroupId) ?? settings.upstreamGroups[0];
    const forwardTag = tagMap.upstreamGroups[group.id];
    if (!forwardTag) continue;
    const tag = standardTag("path", path.id);
    const sequence = buildPathSequence(path, settings, forwardTag, tagMap);
    if (pushIfSupported("path_sequence", "executor", "sequence", tag, sequence)) {
      tagMap.paths[path.id] = tag;
    }
  }

  const defaultPath = settings.paths[0];
  const defaultPathTag =
    tagMap.paths[defaultPath?.id ?? "default"] ??
    Object.values(tagMap.paths)[0] ??
    Object.values(tagMap.upstreamGroups)[0];
  const mainSequence: Array<Record<string, unknown>> = [];
  if (tagMap.system.includes("standard_metrics")) {
    mainSequence.push({ exec: "$standard_metrics" });
  }
  if (defaultPathTag) {
    mainSequence.push({ exec: `$${defaultPathTag}` });
  }
  mainSequence.push({ exec: "accept" });

  const hasMainSequence = pushIfSupported(
    "main_sequence",
    "executor",
    "sequence",
    "standard_main_sequence",
    mainSequence,
  );

  if (hasMainSequence && settings.listen.udp) {
    pushIfSupported("udp_server", "server", "udp_server", "standard_udp", {
      listen: settings.listen.address,
      entry: "standard_main_sequence",
    });
  }
  if (hasMainSequence && settings.listen.tcp) {
    pushIfSupported("tcp_server", "server", "tcp_server", "standard_tcp", {
      listen: settings.listen.address,
      entry: "standard_main_sequence",
    });
  }

  const config: OxiDnsConfig = {
    ...(baseConfig?.api ? { api: baseConfig.api } : {}),
    log: {
      level: settings.system.logLevel,
    },
    runtime: {
      ...(settings.system.threads ? { threads: settings.system.threads } : {}),
    },
    plugins: generated,
  };

  return {
    config,
    skippedCapabilities: Array.from(new Set(skippedCapabilities)),
    generatedTags: generated.map((item) => item.tag),
    tagMap,
    summary: summarizeSettings(settings),
  };
}

function buildPathSequence(
  path: StandardResolutionPath,
  settings: StandardModeSettings,
  forwardTag: string,
  tagMap: StandardTagMap,
): Array<Record<string, unknown>> {
  const sequence: Array<Record<string, unknown>> = [];
  const filteringEnabled =
    path.filtering === "enabled" ||
    (path.filtering === "inherit" && settings.filtering.enabled);
  const cacheEnabled =
    path.cache === "enabled" || (path.cache === "inherit" && settings.cache.enabled);
  const queryLogEnabled =
    path.queryLog === "enabled" ||
    (path.queryLog === "inherit" && settings.queryLog.enabled);

  if (queryLogEnabled && tagMap.queryLog) {
    sequence.push({ exec: `$${tagMap.queryLog}` });
  }
  if (
    filteringEnabled &&
    tagMap.filtering?.includes("standard_ad_rules") &&
    tagMap.filtering?.includes("standard_blocked")
  ) {
    sequence.push({
      matches: "qname $standard_ad_rules",
      exec: "$standard_blocked",
    });
  }
  if (cacheEnabled && tagMap.cache) {
    sequence.push({ exec: `$${tagMap.cache}` });
  }
  sequence.push({ matches: "!has_resp", exec: `$${forwardTag}` });
  return sequence;
}

function summarizeSettings(settings: StandardModeSettings): StandardGenerationSummary {
  return {
    upstreamGroupCount: settings.upstreamGroups.length,
    pathCount: settings.paths.length,
    enabledUpstreamCount: settings.upstreamGroups.reduce(
      (sum, group) =>
        sum + group.upstreams.filter((upstream) => upstream.enabled).length,
      0,
    ),
    filteringEnabled: settings.filtering.enabled,
    cacheEnabled: settings.cache.enabled,
    queryLogEnabled: settings.queryLog.enabled,
    routingRuleCount: settings.routing.rules.filter((rule) => rule.enabled).length,
    exceptionRuleCount: settings.exceptions.filter((rule) => rule.enabled).length,
    deviceCount: settings.devices.length,
  };
}
