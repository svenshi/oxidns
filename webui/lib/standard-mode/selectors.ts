import type { BuildInfo } from "../oxidns-api";
import type {
  OxiDnsConfig,
  OxiDnsPluginConfig as PluginConfig,
} from "../oxidns-config";
import { isPluginKindSupported } from "../build-capabilities";
import { createDefaultStandardSettings } from "./defaults";
import type {
  StandardDualStackSettings,
  StandardIpSelectionSettings,
  StandardModeSettings,
  StandardUpstream,
} from "./types";

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function asStringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.map((item) => String(item)).filter(Boolean)
    : [];
}

function asNumber(value: unknown, fallback: number): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function findPlugin(config: OxiDnsConfig, tag: string): PluginConfig | null {
  return config.plugins.find((plugin) => plugin.tag === tag) ?? null;
}

function parseUpstreams(
  value: unknown,
  group: "global" | "domestic",
): StandardUpstream[] {
  if (!Array.isArray(value)) return [];
  return value
    .map((item, index) => {
      const record = asRecord(item);
      const address = String(record.addr ?? record.address ?? "").trim();
      if (!address) return null;
      const id = String(record.tag ?? `${group}_${index + 1}`);
      return {
        id,
        name: id,
        address,
        ...(record.bootstrap !== undefined
          ? { bootstrap: String(record.bootstrap) }
          : {}),
        enabled: true,
        group,
      };
    })
    .filter((item): item is StandardUpstream => item !== null);
}

function parseDualStack(config: OxiDnsConfig): StandardDualStackSettings {
  if (findPlugin(config, "standard_prefer_ipv4")) {
    return { strategy: "prefer_ipv4" };
  }
  if (findPlugin(config, "standard_prefer_ipv6")) {
    return { strategy: "prefer_ipv6" };
  }
  return { strategy: "auto" };
}

function parseIpSelection(config: OxiDnsConfig): StandardIpSelectionSettings {
  const plugin = findPlugin(config, "standard_ip_selector");
  if (!plugin) return { enabled: false, strategy: "lowest_latency" };
  const args = asRecord(plugin.args);
  const strategy = String(args.strategy ?? "lowest_latency");
  return {
    enabled: true,
    strategy: strategy === "prefer_cn" ? "prefer_cn" : "lowest_latency",
  };
}

export function parseStandardSettingsFromYaml(
  config: OxiDnsConfig,
): StandardModeSettings {
  const defaults = createDefaultStandardSettings();
  if (!config.plugins.some((plugin) => plugin.tag.startsWith("standard_"))) {
    return defaults;
  }

  const udp = findPlugin(config, "standard_udp");
  const tcp = findPlugin(config, "standard_tcp");
  const globalForward = findPlugin(config, "standard_forward_global");
  const domesticForward = findPlugin(config, "standard_forward_domestic");
  const cache = findPlugin(config, "standard_cache");
  const recorder = findPlugin(config, "standard_recorder");
  const adRules = findPlugin(config, "standard_ad_rules");
  const domesticDomains = findPlugin(config, "standard_domestic_domains");
  const log = asRecord(config.log);
  const runtime = asRecord(config.runtime);

  const udpArgs = asRecord(udp?.args);
  const tcpArgs = asRecord(tcp?.args);
  const listen = String(
    udpArgs.listen ?? tcpArgs.listen ?? defaults.listen.address,
  );

  const globalForwardArgs = asRecord(globalForward?.args);
  const parsedGlobalUpstreams = parseUpstreams(
    globalForwardArgs.upstreams,
    "global",
  );
  const domesticForwardArgs = asRecord(domesticForward?.args);
  const parsedDomesticUpstreams = parseUpstreams(
    domesticForwardArgs.upstreams,
    "domestic",
  );
  const cacheArgs = asRecord(cache?.args);
  const recorderArgs = asRecord(recorder?.args);
  const adRulesArgs = asRecord(adRules?.args);
  const domesticDomainArgs = asRecord(domesticDomains?.args);

  return {
    ...defaults,
    listen: {
      address: listen,
      udp: Boolean(udp),
      tcp: Boolean(tcp),
    },
    upstreams:
      parsedGlobalUpstreams.length > 0
        ? parsedGlobalUpstreams
        : defaults.upstreams,
    cache: {
      ...defaults.cache,
      enabled: Boolean(cache),
      size: asNumber(cacheArgs.size, defaults.cache.size),
      minTtl: asNumber(cacheArgs.min_ttl, defaults.cache.minTtl),
      maxTtl: asNumber(cacheArgs.max_ttl, defaults.cache.maxTtl),
      negativeTtl: asNumber(cacheArgs.negative_ttl, defaults.cache.negativeTtl),
    },
    queryLog: {
      ...defaults.queryLog,
      enabled: Boolean(recorder),
      retentionDays: asNumber(
        recorderArgs.retention_days,
        defaults.queryLog.retentionDays,
      ),
      sampleRate: asNumber(
        recorderArgs.sample_rate,
        defaults.queryLog.sampleRate,
      ),
    },
    adBlock: {
      ...defaults.adBlock,
      enabled: Boolean(adRules),
      inlineRules: asStringArray(adRulesArgs.rules),
    },
    split: {
      ...defaults.split,
      enabled: Boolean(domesticDomains || domesticForward),
      domesticDomains: asStringArray(domesticDomainArgs.rules),
      domesticUpstreams: parsedDomesticUpstreams,
    },
    dualStack: parseDualStack(config),
    ipSelection: parseIpSelection(config),
    system: {
      logLevel:
        log.level === "trace" ||
        log.level === "debug" ||
        log.level === "warn" ||
        log.level === "error"
          ? log.level
          : "info",
      threads:
        typeof runtime.threads === "number"
          ? runtime.threads
          : typeof runtime.worker_threads === "number"
            ? runtime.worker_threads
            : undefined,
    },
  };
}

export function selectStandardCapabilityMap(buildInfo: BuildInfo | null) {
  return {
    cache: isPluginKindSupported(buildInfo, "executor", "cache"),
    queryRecorder: isPluginKindSupported(
      buildInfo,
      "executor",
      "query_recorder",
    ),
    adRules: isPluginKindSupported(buildInfo, "provider", "adguard_rule"),
    domainSet: isPluginKindSupported(buildInfo, "provider", "domain_set"),
    forward: isPluginKindSupported(buildInfo, "executor", "forward"),
    ipSelector: isPluginKindSupported(buildInfo, "executor", "ip_selector"),
    preferIpv4: isPluginKindSupported(buildInfo, "executor", "prefer_ipv4"),
    preferIpv6: isPluginKindSupported(buildInfo, "executor", "prefer_ipv6"),
    upgrade: isPluginKindSupported(buildInfo, "executor", "plugin_upgrade"),
  };
}

export function selectStandardSummary(
  config: OxiDnsConfig | null,
  settings: StandardModeSettings | null,
) {
  const standardPlugins = (config?.plugins ?? []).filter((plugin) =>
    plugin.tag.startsWith("standard_"),
  );
  return {
    standardPluginCount: standardPlugins.length,
    upstreamCount:
      settings?.upstreams.filter((item) => item.enabled).length ?? 0,
    cacheEnabled: Boolean(settings?.cache.enabled),
    adBlockEnabled: Boolean(settings?.adBlock.enabled),
    splitEnabled: Boolean(settings?.split.enabled),
  };
}
