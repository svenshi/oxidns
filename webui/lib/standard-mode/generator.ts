import { isPluginKindSupported } from "../build-capabilities";
import type { BuildInfo } from "../oxidns-api";
import type {
  OxiDnsConfig,
  OxiDnsPluginConfig as PluginConfig,
} from "../oxidns-config";
import type { PluginType } from "../types";
import type {
  StandardDeviceProfile,
  StandardExceptionRule,
  StandardGenerationResult,
  StandardGenerationSummary,
  StandardModeSettings,
  StandardResolutionPath,
  StandardRoutingRule,
  StandardServerProtocol,
  StandardServerSettings,
  StandardSubscription,
  StandardTagMap,
  StandardUpstream,
} from "./types";
import {
  normalizeStandardExceptionSettings,
  normalizeStandardDeviceSettings,
  normalizeStandardDnsSettings,
  normalizeStandardFilteringSettings,
  normalizeStandardRoutingSettings,
  isStandardServerProtocolSupported,
  standardFilteringCapabilityMap,
} from "./validation";

export const STANDARD_PLUGIN_TAGS = [
  "standard_recorder",
  "standard_cache",
  "standard_filter_download",
  "standard_ad_rules",
  "standard_blocked",
  "standard_filter_reload",
  "standard_filter_cron",
  "standard_forward_default",
  "standard_path_default",
  "standard_main_sequence",
  "standard_udp",
  "standard_tcp",
  "standard_dot",
  "standard_doh",
  "standard_doq",
] as const;

const STANDARD_TAG_SET = new Set<string>(STANDARD_PLUGIN_TAGS);
const STANDARD_FILTER_SUBSCRIPTION_DIR = "./data/standard-filter-subscriptions";

function plugin(kind: string, tag: string, args?: unknown): PluginConfig {
  return { type: kind, tag, args: args ?? {} };
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
  concurrent: number,
): number {
  return Math.max(1, Math.min(3, upstreams.length, Math.trunc(concurrent)));
}

function blockMode(settings: StandardModeSettings) {
  if (settings.filtering.blockResponse === "nxdomain") return "nxdomain";
  if (settings.filtering.blockResponse === "refused") return "refused";
  return "null";
}

function subscriptionFileName(subscription: StandardSubscription): string {
  const safe = subscription.id
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "_")
    .replace(/^_+|_+$/g, "");
  return `${safe || "subscription"}.txt`;
}

function subscriptionFilePath(subscription: StandardSubscription): string {
  return `${STANDARD_FILTER_SUBSCRIPTION_DIR}/${subscriptionFileName(subscription)}`;
}

function enabledSubscriptions(settings: StandardModeSettings) {
  return settings.filtering.subscriptions.filter(
    (subscription) => subscription.enabled && subscription.url.trim(),
  );
}

function filteringRules(settings: StandardModeSettings) {
  return [
    ...settings.filtering.blockRules,
    ...settings.filtering.allowRules,
  ].filter(Boolean);
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
  const generationSettings = normalizeStandardExceptionSettings(
    normalizeStandardDeviceSettings(
      normalizeStandardRoutingSettings(
        normalizeStandardFilteringSettings(normalizeStandardDnsSettings(settings)),
      ),
    ),
  );
  const generated: PluginConfig[] = [];
  const skippedCapabilities: string[] = [];
  const tagMap: StandardTagMap = {
    upstreamGroups: {},
    paths: {},
    routingRules: {},
    exceptionRules: {},
    devices: {},
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
    generated.push(plugin(kind, tag, args));
    return true;
  };

  const pushServerIfSupported = (
    capability: string,
    server: StandardServerSettings,
    kind: string,
    tag: string,
    args?: unknown,
  ) => {
    if (
      !isStandardServerProtocolSupported(
        server.protocol,
        buildInfo,
        server,
      )
    ) {
      skippedCapabilities.push(capability);
      return false;
    }
    return pushIfSupported(capability, "server", kind, tag, args);
  };

  if (
    shouldEnableQueryLogPlugin(generationSettings) &&
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
        retention_days: Math.max(1, generationSettings.queryLog.retentionDays),
        cleanup_interval_hours: 1,
      },
    )
  ) {
    tagMap.queryLog = "standard_recorder";
  }

  if (
    generationSettings.cache.enabled &&
    pushIfSupported("cache", "executor", "cache", "standard_cache", {
      size: generationSettings.cache.size,
      min_ttl: generationSettings.cache.minTtl,
      max_ttl: generationSettings.cache.maxTtl,
      negative_ttl: generationSettings.cache.negativeTtl,
      short_circuit: true,
    })
  ) {
    tagMap.cache = "standard_cache";
  }

  const filteringSettings = generationSettings;
  const filteringCapabilities = standardFilteringCapabilityMap(buildInfo);
  const subscriptions = enabledSubscriptions(filteringSettings);
  const shouldGenerateFiltering = shouldEnableFilteringPlugins(generationSettings);
  const hasBlockExceptions = generationSettings.exceptions.some(
    (exception) => exception.enabled && exception.action.type === "block",
  );
  const shouldGenerateSubscriptions =
    shouldGenerateFiltering &&
    subscriptions.length > 0 &&
    filteringCapabilities.subscriptionRuntime;
  const subscriptionFiles = shouldGenerateSubscriptions
    ? subscriptions.map(subscriptionFilePath)
    : [];

  if (
    shouldGenerateFiltering &&
    subscriptions.length > 0 &&
    !filteringCapabilities.subscriptionRuntime
  ) {
    skippedCapabilities.push("filter_subscriptions");
  }

  if (
    shouldGenerateSubscriptions &&
    pushIfSupported(
      "filter_download",
      "executor",
      "download",
      "standard_filter_download",
      {
        startup_if_missing: true,
        downloads: subscriptions.map((subscription) => ({
          url: subscription.url,
          dir: STANDARD_FILTER_SUBSCRIPTION_DIR,
          filename: subscriptionFileName(subscription),
        })),
      },
    )
  ) {
    tagMap.filtering = [...(tagMap.filtering ?? []), "standard_filter_download"];
  }

  const rules = filteringRules(filteringSettings);
  const hasFilteringRules =
    shouldGenerateFiltering && (rules.length > 0 || subscriptionFiles.length > 0);
  if (
    hasFilteringRules &&
    pushIfSupported(
      "ad_rules",
      "provider",
      "adguard_rule",
      "standard_ad_rules",
      {
        files: subscriptionFiles,
        rules,
      },
    )
  ) {
    tagMap.filtering = [...(tagMap.filtering ?? []), "standard_ad_rules"];
  }
  if (
    (hasFilteringRules || hasBlockExceptions) &&
    pushIfSupported(
      "black_hole",
      "executor",
      "black_hole",
      "standard_blocked",
      {
        mode: blockMode(filteringSettings),
        short_circuit: true,
      },
    )
  ) {
    tagMap.filtering = [...(tagMap.filtering ?? []), "standard_blocked"];
  }

  const needsPreferIpv4 = generationSettings.exceptions.some(
    (exception) => exception.enabled && exception.action.type === "prefer_ipv4",
  );
  const needsPreferIpv6 = generationSettings.exceptions.some(
    (exception) => exception.enabled && exception.action.type === "prefer_ipv6",
  );
  if (needsPreferIpv4) {
    pushIfSupported(
      "prefer_ipv4",
      "executor",
      "prefer_ipv4",
      "standard_prefer_ipv4",
      {
        cache: true,
        cache_ttl: 3600,
      },
    );
  }
  if (needsPreferIpv6) {
    pushIfSupported(
      "prefer_ipv6",
      "executor",
      "prefer_ipv6",
      "standard_prefer_ipv6",
      {
        cache: true,
        cache_ttl: 3600,
      },
    );
  }

  if (
    shouldGenerateSubscriptions &&
    tagMap.filtering?.includes("standard_ad_rules") &&
    pushIfSupported(
      "filter_reload",
      "executor",
      "reload_provider",
      "standard_filter_reload",
      ["$standard_ad_rules"],
    )
  ) {
    tagMap.filtering = [...(tagMap.filtering ?? []), "standard_filter_reload"];
  }

  const canScheduleFilterRefresh =
    shouldGenerateSubscriptions &&
    tagMap.filtering?.includes("standard_filter_download") &&
    tagMap.filtering?.includes("standard_filter_reload");
  if (
    canScheduleFilterRefresh &&
    pushIfSupported("filter_cron", "executor", "cron", "standard_filter_cron", {
      jobs: [
        {
          name: "refresh_filter_subscriptions",
          interval: `${Math.max(
            1,
            Math.min(...subscriptions.map((item) => item.updateIntervalHours)),
          )}h`,
          executors: ["$standard_filter_download", "$standard_filter_reload"],
        },
      ],
    })
  ) {
    tagMap.filtering = [...(tagMap.filtering ?? []), "standard_filter_cron"];
  }

  const groupsById = new Map(
    generationSettings.upstreamGroups.map((group) => [group.id, group]),
  );
  for (const group of generationSettings.upstreamGroups) {
    const upstreams = enabledUpstreams(group.upstreams);
    if (upstreams.length === 0) continue;
    const tag = standardTag("forward", group.id);
    if (
      pushIfSupported("forward", "executor", "forward", tag, {
        upstreams,
        concurrent: concurrentCount(upstreams, group.concurrent),
      })
    ) {
      tagMap.upstreamGroups[group.id] = tag;
    }
  }

  for (const path of generationSettings.paths) {
    const group =
      groupsById.get(path.upstreamGroupId) ?? generationSettings.upstreamGroups[0];
    const forwardTag = tagMap.upstreamGroups[group.id];
    if (!forwardTag) continue;
    const tag = standardTag("path", path.id);
    const sequence = buildPathSequence(path, generationSettings, forwardTag, tagMap);
    if (pushIfSupported("path_sequence", "executor", "sequence", tag, sequence)) {
      tagMap.paths[path.id] = tag;
    }
  }

  const defaultPath = generationSettings.paths[0];
  const defaultPathTag =
    tagMap.paths[defaultPath?.id ?? "default"] ??
    Object.values(tagMap.paths)[0] ??
    Object.values(tagMap.upstreamGroups)[0];
  for (const exception of generationSettings.exceptions) {
    if (!exception.enabled) continue;
    const matcher = ruleMatcher(exception);
    if (!matcher) continue;
    const tag = standardTag("exception_match", exception.id);
    if (
      pushIfSupported(
        `exception_${matcher.kind}`,
        "matcher",
        matcher.kind,
        tag,
        matcher.args,
      )
    ) {
      tagMap.exceptionRules[exception.id] = tag;
    }
  }

  const exceptionActionTags = new Map<string, string>();
  for (const exception of generationSettings.exceptions) {
    if (!exception.enabled || !tagMap.exceptionRules[exception.id]) continue;
    if (
      exception.action.type === "use_path" ||
      exception.action.type === "use_default_path"
    ) {
      continue;
    }
    if (
      exception.action.type === "prefer_ipv4" &&
      !generated.some((item) => item.tag === "standard_prefer_ipv4")
    ) {
      continue;
    }
    if (
      exception.action.type === "prefer_ipv6" &&
      !generated.some((item) => item.tag === "standard_prefer_ipv6")
    ) {
      continue;
    }
    const sequence = buildExceptionSequence(
      exception,
      generationSettings,
      defaultPath,
      defaultPathTag,
      tagMap,
    );
    if (!sequence) continue;
    const tag = standardTag("exception_action", exception.id);
    if (
      pushIfSupported(
        `exception_action_${exception.action.type}`,
        "executor",
        "sequence",
        tag,
        sequence,
      )
    ) {
      exceptionActionTags.set(exception.id, tag);
    }
  }

  const deviceActionTags = new Map<string, string>();
  for (const device of generationSettings.devices) {
    if (!deviceHasPolicy(device)) continue;
    const matcher = deviceMatcher(device);
    if (!matcher) continue;
    const tag = standardTag("device_match", device.id);
    if (
      pushIfSupported("device_client_ip", "matcher", "client_ip", tag, matcher.args)
    ) {
      tagMap.devices = {
        ...(tagMap.devices ?? {}),
        [device.id]: tag,
      };
    }
  }

  for (const device of generationSettings.devices) {
    if (!deviceHasPolicy(device) || !tagMap.devices?.[device.id]) continue;
    const sequence = buildDeviceSequence(
      device,
      generationSettings,
      defaultPath,
      defaultPathTag,
      tagMap,
    );
    if (!sequence) continue;
    const tag = standardTag("device_action", device.id);
    if (
      pushIfSupported("device_action_sequence", "executor", "sequence", tag, sequence)
    ) {
      deviceActionTags.set(device.id, tag);
    }
  }

  if (generationSettings.routing.enabled) {
    for (const rule of generationSettings.routing.rules) {
      if (!rule.enabled) continue;
      const route = ruleMatcher(rule);
      const targetPathTag = routeTargetPathTag(rule, defaultPathTag, tagMap);
      if (!route || !targetPathTag) continue;
      const tag = standardTag("route_match", rule.id);
      if (
        pushIfSupported(
          `route_${route.kind}`,
          "matcher",
          route.kind,
          tag,
          route.args,
        )
      ) {
        tagMap.routingRules[rule.id] = tag;
      }
    }
  }

  const mainSequence: Array<Record<string, unknown>> = [];
  for (const exception of orderedExceptions(generationSettings.exceptions)) {
    const matchTag = tagMap.exceptionRules[exception.id];
    const execTag = exceptionExecTag(
      exception,
      exceptionActionTags,
      tagMap,
      defaultPathTag,
    );
    if (!exception.enabled || !matchTag || !execTag) continue;
    mainSequence.push({
      matches: `$${matchTag}`,
      exec: `$${execTag}`,
    });
  }
  for (const device of generationSettings.devices) {
    const matchTag = tagMap.devices?.[device.id];
    const execTag = deviceActionTags.get(device.id);
    if (!matchTag || !execTag) continue;
    mainSequence.push({
      matches: `$${matchTag}`,
      exec: `$${execTag}`,
    });
  }
  if (generationSettings.routing.enabled) {
    for (const rule of generationSettings.routing.rules) {
      const matchTag = tagMap.routingRules[rule.id];
      const targetPathTag = routeTargetPathTag(rule, defaultPathTag, tagMap);
      if (!rule.enabled || !matchTag || !targetPathTag) continue;
      mainSequence.push({
        matches: `$${matchTag}`,
        exec: `$${targetPathTag}`,
      });
    }
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

  if (hasMainSequence) {
    for (const server of generationSettings.listen.servers) {
      pushServerIfSupported(
        `${server.protocol}_server`,
        server,
        serverPluginKind(server.protocol),
        serverTag(server),
        serverPluginArgs(server),
      );
    }
  }

  const config: OxiDnsConfig = {
    ...(baseConfig?.api ? { api: baseConfig.api } : {}),
    log: {
      level: generationSettings.system.logLevel,
    },
    runtime: {
      ...(generationSettings.system.threads
        ? { threads: generationSettings.system.threads }
        : {}),
    },
    plugins: generated,
  };

  return {
    config,
    skippedCapabilities: Array.from(new Set(skippedCapabilities)),
    generatedTags: generated.map((item) => item.tag),
    tagMap,
    summary: summarizeSettings(generationSettings),
  };
}

function serverPluginKind(
  protocol: StandardServerProtocol,
): "udp_server" | "tcp_server" | "http_server" | "quic_server" {
  if (protocol === "udp") return "udp_server";
  if (protocol === "doh") return "http_server";
  if (protocol === "doq") return "quic_server";
  return "tcp_server";
}

function serverTag(server: StandardServerSettings): string {
  if (server.id === "udp" && server.protocol === "udp") return "standard_udp";
  if (server.id === "tcp" && server.protocol === "tcp") return "standard_tcp";
  if (server.id === "dot" && server.protocol === "dot") return "standard_dot";
  if (server.id === "doh" && server.protocol === "doh") return "standard_doh";
  if (server.id === "doq" && server.protocol === "doq") return "standard_doq";
  return standardTag("server", server.id);
}

function serverPluginArgs(server: StandardServerSettings): Record<string, unknown> {
  if (server.protocol === "udp") {
    return {
      listen: server.listen,
      entry: "standard_main_sequence",
    };
  }
  if (server.protocol === "doh") {
    return {
      listen: server.listen,
      entries: [
        {
          path: server.path || "/dns-query",
          exec: "standard_main_sequence",
        },
      ],
      cert: server.cert,
      key: server.key,
      ...(server.srcIpHeader ? { src_ip_header: server.srcIpHeader } : {}),
      ...(server.idleTimeout ? { idle_timeout: server.idleTimeout } : {}),
      ...(server.enableHttp3 ? { enable_http3: true } : {}),
    };
  }
  return {
    listen: server.listen,
    entry: "standard_main_sequence",
    ...(server.protocol === "dot" || server.protocol === "doq"
      ? { cert: server.cert, key: server.key }
      : {}),
    ...(server.idleTimeout ? { idle_timeout: server.idleTimeout } : {}),
  };
}

function buildPathSequence(
  path: StandardResolutionPath,
  settings: StandardModeSettings,
  forwardTag: string,
  tagMap: StandardTagMap,
  options: {
    disableFiltering?: boolean;
    forceFiltering?: boolean;
    disableQueryLog?: boolean;
    forceQueryLog?: boolean;
    prependExec?: string;
  } = {},
): Array<Record<string, unknown>> {
  const sequence: Array<Record<string, unknown>> = [];
  const filteringEnabled =
    (!options.disableFiltering && options.forceFiltering) ||
    (!options.disableFiltering && path.filtering === "enabled") ||
    (!options.disableFiltering &&
      path.filtering === "inherit" &&
      settings.filtering.enabled);
  const cacheEnabled =
    path.cache === "enabled" || (path.cache === "inherit" && settings.cache.enabled);
  const queryLogEnabled =
    (!options.disableQueryLog && options.forceQueryLog) ||
    (!options.disableQueryLog && path.queryLog === "enabled") ||
    (!options.disableQueryLog &&
      path.queryLog === "inherit" &&
      settings.queryLog.enabled);

  if (options.prependExec) {
    sequence.push({ exec: options.prependExec });
  }
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
  sequence.push({ exec: "accept" });
  return sequence;
}

function deviceHasPolicy(device: StandardDeviceProfile): boolean {
  return Boolean(
    device.assignedPathId ||
      device.filtering === "enabled" ||
      device.filtering === "disabled" ||
      device.queryLog === "enabled" ||
      device.queryLog === "disabled",
  );
}

function deviceMatcher(
  device: StandardDeviceProfile,
): { kind: "client_ip"; args: string[] } | null {
  const values = device.addresses.map((address) => address.trim()).filter(Boolean);
  if (values.length === 0) return null;
  return { kind: "client_ip", args: values };
}

function buildDeviceSequence(
  device: StandardDeviceProfile,
  settings: StandardModeSettings,
  defaultPath: StandardResolutionPath | undefined,
  defaultPathTag: string | undefined,
  tagMap: StandardTagMap,
): Array<Record<string, unknown>> | null {
  const path = device.assignedPathId
    ? settings.paths.find((item) => item.id === device.assignedPathId) ?? defaultPath
    : defaultPath;
  const forwardTag = path
    ? tagMap.upstreamGroups[path.upstreamGroupId]
    : Object.values(tagMap.upstreamGroups)[0];
  if (!path || !forwardTag || !defaultPathTag) return null;
  return buildPathSequence(path, settings, forwardTag, tagMap, {
    disableFiltering: device.filtering === "disabled",
    forceFiltering: device.filtering === "enabled",
    disableQueryLog: device.queryLog === "disabled",
    forceQueryLog: device.queryLog === "enabled",
  });
}

function ruleMatcher(
  rule: StandardRoutingRule | StandardExceptionRule,
): { kind: "qname" | "client_ip" | "qtype"; args: unknown } | null {
  if (
    rule.condition.type === "domain" ||
    rule.condition.type === "suffix" ||
    rule.condition.type === "keyword"
  ) {
    return {
      kind: "qname",
      args: ruleDomainRules(rule),
    };
  }
  if (rule.condition.type === "client_cidr") {
    return { kind: "client_ip", args: rule.condition.values };
  }
  if (rule.condition.type === "qtype") {
    return {
      kind: "qtype",
      args: rule.condition.values.map((value) => value.toUpperCase()),
    };
  }
  return null;
}

function ruleDomainRules(rule: StandardRoutingRule | StandardExceptionRule): string[] {
  if (rule.condition.type === "domain") {
    return rule.condition.values.map((value) => `full:${value}`);
  }
  if (rule.condition.type === "suffix") {
    return rule.condition.values.map((value) => `domain:${value.replace(/^\.+/, "")}`);
  }
  if (rule.condition.type === "keyword") {
    return rule.condition.values.map((value) => `keyword:${value}`);
  }
  return [];
}

function buildExceptionSequence(
  exception: StandardExceptionRule,
  settings: StandardModeSettings,
  defaultPath: StandardResolutionPath | undefined,
  defaultPathTag: string | undefined,
  tagMap: StandardTagMap,
): Array<Record<string, unknown>> | null {
  const defaultForwardTag = defaultPath
    ? tagMap.upstreamGroups[defaultPath.upstreamGroupId]
    : Object.values(tagMap.upstreamGroups)[0];
  if (exception.action.type === "block") {
    return tagMap.filtering?.includes("standard_blocked")
      ? [{ exec: "$standard_blocked" }, { exec: "accept" }]
      : null;
  }
  if (!defaultPath || !defaultForwardTag) return null;
  if (
    exception.action.type === "allow" ||
    exception.action.type === "skip_filtering"
  ) {
    return buildPathSequence(defaultPath, settings, defaultForwardTag, tagMap, {
      disableFiltering: true,
    });
  }
  if (exception.action.type === "disable_logging") {
    return buildPathSequence(defaultPath, settings, defaultForwardTag, tagMap, {
      disableQueryLog: true,
    });
  }
  if (
    exception.action.type === "prefer_ipv4" &&
    settings.exceptions.some((item) => item.id === exception.id)
  ) {
    return buildPathSequence(defaultPath, settings, defaultForwardTag, tagMap, {
      prependExec: "$standard_prefer_ipv4",
    });
  }
  if (
    exception.action.type === "prefer_ipv6" &&
    settings.exceptions.some((item) => item.id === exception.id)
  ) {
    return buildPathSequence(defaultPath, settings, defaultForwardTag, tagMap, {
      prependExec: "$standard_prefer_ipv6",
    });
  }
  return null;
}

function orderedExceptions(exceptions: StandardExceptionRule[]) {
  const priority: Record<StandardExceptionRule["action"]["type"], number> = {
    block: 0,
    allow: 1,
    skip_filtering: 2,
    use_path: 3,
    use_default_path: 3,
    prefer_ipv4: 4,
    prefer_ipv6: 4,
    disable_logging: 5,
  };
  return [...exceptions].sort(
    (left, right) => priority[left.action.type] - priority[right.action.type],
  );
}

function exceptionExecTag(
  exception: StandardExceptionRule,
  actionTags: Map<string, string>,
  tagMap: StandardTagMap,
  defaultPathTag: string | undefined,
): string | undefined {
  if (exception.action.type === "use_path") {
    return tagMap.paths[exception.action.pathId];
  }
  if (exception.action.type === "use_default_path") return defaultPathTag;
  return actionTags.get(exception.id);
}

function routeTargetPathTag(
  rule: StandardRoutingRule,
  defaultPathTag: string | undefined,
  tagMap: StandardTagMap,
): string | undefined {
  if (rule.action.type === "use_default_path") return defaultPathTag;
  if (rule.action.type === "use_path") return tagMap.paths[rule.action.pathId];
  return undefined;
}

function shouldEnableFilteringPlugins(settings: StandardModeSettings): boolean {
  return (
    settings.filtering.enabled ||
    settings.devices.some((device) => device.filtering === "enabled")
  );
}

function shouldEnableQueryLogPlugin(settings: StandardModeSettings): boolean {
  return (
    settings.queryLog.enabled ||
    settings.devices.some((device) => device.queryLog === "enabled")
  );
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
