import type { BuildInfo } from "../oxidns-api";
import type {
  OxiDnsConfig,
  OxiDnsPluginConfig as PluginConfig,
} from "../oxidns-config";
import { isPluginKindSupported } from "../build-capabilities";
import type { PluginType } from "../types";
import type {
  StandardGenerationResult,
  StandardModeSettings,
  StandardUpstream,
} from "./types";

export const STANDARD_PLUGIN_TAGS = [
  "standard_metrics",
  "standard_recorder",
  "standard_cache",
  "standard_ad_rules",
  "standard_domestic_domains",
  "standard_blocked",
  "standard_forward_global",
  "standard_forward_domestic",
  "standard_prefer_ipv4",
  "standard_prefer_ipv6",
  "standard_ip_selector",
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

function enabledUpstreams(upstreams: StandardUpstream[]) {
  return upstreams
    .filter((item) => item.enabled && item.address.trim())
    .map((item) => ({
      tag: item.id,
      addr: item.address.trim(),
      ...(item.bootstrap ? { bootstrap: item.bootstrap } : {}),
    }));
}

function concurrentCount(
  upstreams: ReturnType<typeof enabledUpstreams>,
): number {
  return Math.max(1, Math.min(3, upstreams.length));
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

  const hasMetrics = pushIfSupported(
    "metrics",
    "executor",
    "metrics_collector",
    "standard_metrics",
    {},
  );

  const hasRecorder =
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
    );

  const hasCache =
    settings.cache.enabled &&
    pushIfSupported("cache", "executor", "cache", "standard_cache", {
      size: settings.cache.size,
      min_ttl: settings.cache.minTtl,
      max_ttl: settings.cache.maxTtl,
      negative_ttl: settings.cache.negativeTtl,
      short_circuit: true,
    });

  const hasAdRules =
    settings.adBlock.enabled &&
    pushIfSupported(
      "ad_rules",
      "provider",
      "adguard_rule",
      "standard_ad_rules",
      {
        files: [],
        rules: settings.adBlock.inlineRules.filter(Boolean),
      },
    );

  const hasDomesticDomains =
    settings.split.enabled &&
    settings.split.domesticDomains.length > 0 &&
    pushIfSupported(
      "domestic_domains",
      "provider",
      "domain_set",
      "standard_domestic_domains",
      {
        rules: settings.split.domesticDomains,
      },
    );

  const globalUpstreams = enabledUpstreams(settings.upstreams);
  const domesticUpstreams = enabledUpstreams(settings.split.domesticUpstreams);

  const hasGlobalForward = pushIfSupported(
    "global_forward",
    "executor",
    "forward",
    "standard_forward_global",
    {
      upstreams:
        globalUpstreams.length > 0
          ? globalUpstreams
          : [{ tag: "default", addr: "1.1.1.1:53" }],
      concurrent: concurrentCount(globalUpstreams),
    },
  );

  const hasDomesticForward =
    settings.split.enabled &&
    domesticUpstreams.length > 0 &&
    pushIfSupported(
      "domestic_forward",
      "executor",
      "forward",
      "standard_forward_domestic",
      {
        upstreams: domesticUpstreams,
        concurrent: concurrentCount(domesticUpstreams),
      },
    );

  const hasPreferIpv4 =
    settings.dualStack.strategy === "prefer_ipv4" &&
    pushIfSupported(
      "prefer_ipv4",
      "executor",
      "prefer_ipv4",
      "standard_prefer_ipv4",
      {},
    );
  const hasPreferIpv6 =
    settings.dualStack.strategy === "prefer_ipv6" &&
    pushIfSupported(
      "prefer_ipv6",
      "executor",
      "prefer_ipv6",
      "standard_prefer_ipv6",
      {},
    );
  const hasIpSelector =
    settings.ipSelection.enabled &&
    pushIfSupported(
      "ip_selector",
      "executor",
      "ip_selector",
      "standard_ip_selector",
      {
        strategy: settings.ipSelection.strategy,
      },
    );

  const sequence: Array<Record<string, unknown>> = [];
  if (hasMetrics) {
    sequence.push({ exec: "$standard_metrics" });
  }
  if (hasRecorder) {
    sequence.push({ exec: "$standard_recorder" });
  }
  if (hasAdRules) {
    sequence.push({
      matches: "qname $standard_ad_rules",
      exec: "black_hole 0.0.0.0 ::",
    });
  }
  if (hasDomesticDomains && hasDomesticForward) {
    sequence.push({
      matches: "qname $standard_domestic_domains",
      exec: "$standard_forward_domestic",
    });
  }
  if (hasPreferIpv4) {
    sequence.push({ exec: "$standard_prefer_ipv4" });
  }
  if (hasPreferIpv6) {
    sequence.push({ exec: "$standard_prefer_ipv6" });
  }
  if (hasIpSelector) {
    sequence.push({ exec: "$standard_ip_selector" });
  }
  if (hasCache) {
    sequence.push({ exec: "$standard_cache" });
  }
  if (hasGlobalForward) {
    sequence.push({ matches: "!has_resp", exec: "$standard_forward_global" });
  }
  sequence.push({ exec: "accept" });

  const hasMainSequence = pushIfSupported(
    "main_sequence",
    "executor",
    "sequence",
    "standard_main_sequence",
    sequence,
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

  const retainedPlugins = (baseConfig?.plugins ?? []).filter(
    (item) => !isStandardManagedPlugin(item.tag),
  );
  const config: OxiDnsConfig = {
    ...(baseConfig ?? {}),
    log: {
      ...(baseConfig?.log ?? {}),
      level: settings.system.logLevel,
    },
    runtime: {
      ...(baseConfig?.runtime ?? {}),
      ...(settings.system.threads ? { threads: settings.system.threads } : {}),
    },
    plugins: [...retainedPlugins, ...generated],
  };

  return {
    config,
    skippedCapabilities: Array.from(new Set(skippedCapabilities)),
    generatedTags: generated.map((item) => item.tag),
  };
}
