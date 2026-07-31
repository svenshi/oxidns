import { createDefaultStandardSettings } from "./defaults";
import type {
  StandardCacheSettings,
  StandardDeviceProfile,
  StandardExceptionRule,
  StandardFilterFile,
  StandardFilteringSettings,
  StandardLocalSettings,
  StandardModeSettings,
  StandardQueryLogSettings,
  StandardResolutionPath,
  StandardRuleDataRole,
  StandardRuleDataSettings,
  StandardRuleDataSource,
  StandardRoutingRule,
  StandardRoutingSettings,
  StandardScenario,
  StandardSubscription,
  StandardSystemSettings,
  StandardSmartRoutingSettings,
  StandardUpstream,
  StandardUpstreamGroup,
  StandardUpstreamProtocol,
} from "./types";

export type StandardSettingsNotice =
  | "legacy_migrated"
  | "invalid_fallback"
  | null;

export interface StandardSettingsLoadResult {
  settings: StandardModeSettings;
  notice: StandardSettingsNotice;
}

const upstreamProtocols = new Set<StandardUpstreamProtocol>([
  "auto",
  "udp",
  "tcp",
  "dot",
  "doh",
  "doh3",
  "doq",
]);

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function asString(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function asBoolean(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function asNumber(value: unknown, fallback: number): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function optionalPositiveNumber<K extends string>(
  key: K,
  value: unknown,
  allowZero = false,
): Partial<Record<K, number>> {
  const parsed = Number(value);
  const minimum = allowZero ? 0 : 1;
  if (!Number.isFinite(parsed) || parsed < minimum) return {};
  return { [key]: Math.floor(parsed) } as Partial<Record<K, number>>;
}

function optionalNonNegativeNumber<K extends string>(
  key: K,
  value: unknown,
): Partial<Record<K, number>> {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed < 0) return {};
  return { [key]: Math.floor(parsed) } as Partial<Record<K, number>>;
}

function asStringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.map((item) => String(item).trim()).filter(Boolean)
    : [];
}

function cleanId(value: unknown, fallback: string): string {
  const raw = String(value ?? "").trim();
  return raw
    ? raw
        .toLowerCase()
        .replace(/[^a-z0-9_-]+/g, "_")
        .replace(/^_+|_+$/g, "") || fallback
    : fallback;
}

function normalizeUpstream(value: unknown, index: number): StandardUpstream {
  const source = asRecord(value);
  const address = asString(source.address ?? source.addr).trim();
  const protocol = upstreamProtocols.has(
    source.protocol as StandardUpstreamProtocol,
  )
    ? (source.protocol as StandardUpstreamProtocol)
    : "auto";
  const id = cleanId(source.id ?? source.tag, `upstream_${index + 1}`);
  return {
    id,
    name: asString(source.name, id),
    protocol,
    address,
    enabled: asBoolean(source.enabled, true),
    ...(asString(source.bootstrap).trim()
      ? { bootstrap: asString(source.bootstrap).trim() }
      : {}),
    ...(source.bootstrapVersion === 4 || source.bootstrapVersion === 6
      ? { bootstrapVersion: source.bootstrapVersion }
      : {}),
    ...(asString(source.dialAddress ?? source.dial_addr).trim()
      ? { dialAddress: asString(source.dialAddress ?? source.dial_addr).trim() }
      : {}),
    ...(asString(source.outbound).trim()
      ? { outbound: asString(source.outbound).trim() }
      : {}),
    ...(asString(source.socks5).trim()
      ? { socks5: asString(source.socks5).trim() }
      : {}),
    ...optionalPositiveNumber("timeoutSeconds", source.timeoutSeconds),
    ...optionalPositiveNumber("idleTimeoutSeconds", source.idleTimeoutSeconds),
    ...optionalPositiveNumber("maxConns", source.maxConns),
    ...optionalPositiveNumber("minConns", source.minConns, true),
    ...(typeof source.enablePipeline === "boolean"
      ? { enablePipeline: source.enablePipeline }
      : {}),
    ...(typeof source.tlsVerify === "boolean"
      ? { tlsVerify: source.tlsVerify }
      : { tlsVerify: true }),
    ...(asString(source.dohPath).trim()
      ? { dohPath: asString(source.dohPath).trim() }
      : {}),
    ...(typeof source.enableHttp3 === "boolean"
      ? { enableHttp3: source.enableHttp3 }
      : {}),
  };
}

function normalizeUpstreamGroup(
  value: unknown,
  index: number,
): StandardUpstreamGroup | null {
  const defaults = createDefaultStandardSettings();
  const source = asRecord(value);
  const id = cleanId(source.id, index === 0 ? "default" : `group_${index + 1}`);
  const upstreams = Array.isArray(source.upstreams)
    ? source.upstreams.map((item, upstreamIndex) =>
        normalizeUpstream(item, upstreamIndex),
      )
    : [];
  const strategy =
    source.strategy === "fastest" ||
    source.strategy === "balanced" ||
    source.strategy === "prefer_positive" ||
    source.strategy === "consensus" ||
    source.strategy === "ordered_fallback"
      ? source.strategy
      : "balanced";
  return {
    id,
    name: asString(
      source.name,
      id === "default" ? defaults.upstreamGroups[0].name : id,
    ),
    ...(asString(source.description).trim()
      ? { description: asString(source.description).trim() }
      : {}),
    strategy,
    upstreams:
      upstreams.length > 0 || id !== "default"
        ? upstreams
        : defaults.upstreamGroups[0].upstreams,
    ...(source.isDefault === true || id === "default"
      ? { isDefault: true }
      : {}),
  };
}

function normalizePath(
  value: unknown,
  index: number,
): StandardResolutionPath | null {
  const defaults = createDefaultStandardSettings();
  const source = asRecord(value);
  const id = cleanId(source.id, index === 0 ? "default" : `path_${index + 1}`);
  const filtering =
    source.filtering === "enabled" || source.filtering === "disabled"
      ? source.filtering
      : "inherit";
  const cache =
    source.cache === "enabled" || source.cache === "disabled"
      ? source.cache
      : "inherit";
  const queryLog =
    source.queryLog === "enabled" || source.queryLog === "disabled"
      ? source.queryLog
      : "inherit";
  const dualStack =
    source.dualStack === "disabled" ||
    source.dualStack === "prefer_ipv4" ||
    source.dualStack === "prefer_ipv6" ||
    source.dualStack === "ipv4_only" ||
    source.dualStack === "ipv6_only"
      ? source.dualStack
      : "inherit";
  const defaultIpSelection = defaults.paths[0].ipSelection;
  const ipSource = asRecord(source.ipSelection);
  const legacyIpSelection =
    source.ipSelection === "enabled" || source.ipSelection === "disabled"
      ? source.ipSelection
      : "inherit";
  const selectionMode =
    ipSource.selectionMode === "best_within_budget" ||
    ipSource.selectionMode === "background"
      ? ipSource.selectionMode
      : "first_success";
  const dnssecPolicy =
    ipSource.dnssecPolicy === "skip" ? "skip" : "reorder_only";
  const ipSelection = {
    ...defaultIpSelection,
    enabled:
      typeof ipSource.enabled === "boolean"
        ? ipSource.enabled
        : legacyIpSelection === "enabled",
    selectionMode,
    probeMethods:
      asStringArray(ipSource.probeMethods).length > 0
        ? asStringArray(ipSource.probeMethods)
        : defaultIpSelection.probeMethods,
    probeStaggerMs: Math.max(
      0,
      asNumber(ipSource.probeStaggerMs, defaultIpSelection.probeStaggerMs),
    ),
    probeTimeoutMs: Math.max(
      1,
      asNumber(ipSource.probeTimeoutMs, defaultIpSelection.probeTimeoutMs),
    ),
    maxWaitMs: Math.max(
      1,
      asNumber(ipSource.maxWaitMs, defaultIpSelection.maxWaitMs),
    ),
    topN: Math.max(1, asNumber(ipSource.topN, defaultIpSelection.topN)),
    ...(asString(ipSource.outbound).trim()
      ? { outbound: asString(ipSource.outbound).trim() }
      : {}),
    ...(asString(ipSource.socks5).trim()
      ? { socks5: asString(ipSource.socks5).trim() }
      : {}),
    dnssecPolicy,
    maxParallelProbes: Math.max(
      1,
      asNumber(
        ipSource.maxParallelProbes,
        defaultIpSelection.maxParallelProbes,
      ),
    ),
    cacheEnabled: asBoolean(
      ipSource.cacheEnabled,
      defaultIpSelection.cacheEnabled,
    ),
    cacheSize: Math.max(
      1,
      asNumber(ipSource.cacheSize, defaultIpSelection.cacheSize),
    ),
    cacheTtlSeconds: Math.max(
      1,
      asNumber(
        ipSource.cacheTtlSeconds,
        defaultIpSelection.cacheTtlSeconds,
      ),
    ),
    failureTtlSeconds: Math.max(
      1,
      asNumber(
        ipSource.failureTtlSeconds,
        defaultIpSelection.failureTtlSeconds,
      ),
    ),
  } satisfies StandardResolutionPath["ipSelection"];
  const ecsSource = asRecord(source.ecs);
  const ecsMode = asString(ecsSource.mode);
  const mask4 = Math.min(32, Math.max(0, asNumber(ecsSource.mask4, 24)));
  const mask6 = Math.min(128, Math.max(0, asNumber(ecsSource.mask6, 48)));
  const ecs: StandardResolutionPath["ecs"] =
    ecsMode === "preset"
      ? {
          mode: "preset",
          address: asString(ecsSource.address).trim(),
          mask4,
          mask6,
        }
      : ecsMode === "client_subnet"
        ? { mode: "client_subnet", mask4, mask6 }
        : ecsMode === "remove" || source.ecs === "disabled"
          ? { mode: "remove" }
          : ecsMode === "preserve_client"
            ? { mode: "preserve_client" }
            : source.ecs === "enabled"
              ? { mode: "client_subnet", mask4: 24, mask6: 48 }
              : { mode: "inherit" };
  return {
    id,
    name: asString(source.name, id === "default" ? defaults.paths[0].name : id),
    ...(asString(source.description).trim()
      ? { description: asString(source.description).trim() }
      : {}),
    upstreamGroupId: asString(source.upstreamGroupId).trim(),
    filtering,
    cache,
    queryLog,
    dualStack,
    ipSelection,
    ecs,
  };
}

function normalizeCache(value: unknown): StandardCacheSettings {
  const defaults = createDefaultStandardSettings().cache;
  const source = asRecord(value);
  return {
    enabled: asBoolean(source.enabled, defaults.enabled),
    size: Math.max(128, asNumber(source.size, defaults.size)),
    minPositiveTtl: Math.max(
      0,
      asNumber(source.minPositiveTtl ?? source.minTtl, defaults.minPositiveTtl),
    ),
    maxPositiveTtl: Math.max(
      0,
      asNumber(source.maxPositiveTtl ?? source.maxTtl, defaults.maxPositiveTtl),
    ),
    maxNegativeTtl: Math.max(
      0,
      asNumber(
        source.maxNegativeTtl ?? source.negativeTtl,
        defaults.maxNegativeTtl,
      ),
    ),
    negativeTtlWithoutSoa: Math.max(
      0,
      asNumber(
        source.negativeTtlWithoutSoa ?? source.negativeTtl,
        defaults.negativeTtlWithoutSoa,
      ),
    ),
  };
}

function normalizeQueryLog(value: unknown): StandardQueryLogSettings {
  const defaults = createDefaultStandardSettings().queryLog;
  const source = asRecord(value);
  return {
    enabled: asBoolean(source.enabled, defaults.enabled),
    retentionDays: Math.max(
      1,
      asNumber(source.retentionDays, defaults.retentionDays),
    ),
    sampleRate: Math.min(
      1,
      Math.max(0, asNumber(source.sampleRate, defaults.sampleRate)),
    ),
  };
}

function normalizeFiltering(value: unknown): StandardFilteringSettings {
  const defaults = createDefaultStandardSettings().filtering;
  const source = asRecord(value);
  const blockResponse =
    source.blockResponse === "nxdomain" ||
    source.blockResponse === "nodata" ||
    source.blockResponse === "refused"
      ? source.blockResponse
      : "null_ip";
  return {
    enabled: asBoolean(source.enabled, defaults.enabled),
    subscriptions: Array.isArray(source.subscriptions)
      ? source.subscriptions
          .map(normalizeSubscription)
          .filter((item): item is StandardSubscription => item !== null)
      : [],
    localFiles: Array.isArray(source.localFiles)
      ? source.localFiles.map(normalizeFilterFile)
      : [],
    blockRules: asStringArray(source.blockRules),
    allowRules: asStringArray(source.allowRules),
    blockResponse,
  };
}

function normalizeFilterFile(
  value: unknown,
  index: number,
): StandardFilterFile {
  const source = asRecord(value);
  const id = cleanId(source.id, `filter_file_${index + 1}`);
  return {
    id,
    name: asString(source.name, id),
    path: asString(source.path).trim(),
    enabled: asBoolean(source.enabled, true),
  };
}

function normalizeLocal(value: unknown): StandardLocalSettings {
  const defaults = createDefaultStandardSettings().local;
  const source = asRecord(value);
  const hosts = asRecord(source.hosts);
  const redirects = asRecord(source.redirects);
  const records = asRecord(source.records);
  const responseTtl = asRecord(source.responseTtl);
  const qtypePolicy = asRecord(source.qtypePolicy);
  const ddns = asRecord(source.ddns);
  const response =
    qtypePolicy.response === "null_ip" ||
    qtypePolicy.response === "nxdomain" ||
    qtypePolicy.response === "nodata" ||
    qtypePolicy.response === "refused"
      ? qtypePolicy.response
      : defaults.qtypePolicy.response;
  return {
    hosts: {
      entries: asStringArray(hosts.entries),
      files: asStringArray(hosts.files),
    },
    redirects: {
      rules: asStringArray(redirects.rules),
      files: asStringArray(redirects.files),
    },
    records: {
      rules: asStringArray(records.rules),
      files: asStringArray(records.files),
    },
    responseTtl: {
      enabled: asBoolean(responseTtl.enabled, defaults.responseTtl.enabled),
      ...optionalNonNegativeNumber(
        "min",
        responseTtl.min ?? defaults.responseTtl.min,
      ),
      ...optionalNonNegativeNumber(
        "max",
        responseTtl.max ?? defaults.responseTtl.max,
      ),
    },
    qtypePolicy: {
      enabled: asBoolean(qtypePolicy.enabled, defaults.qtypePolicy.enabled),
      qtypes: asStringArray(qtypePolicy.qtypes).map((qtype) =>
        qtype.toUpperCase(),
      ),
      response,
    },
    ddns: {
      enabled: asBoolean(ddns.enabled, defaults.ddns.enabled),
      domains: asStringArray(ddns.domains),
      ...(asString(ddns.pathId).trim()
        ? { pathId: cleanId(ddns.pathId, "") }
        : {}),
      ttl: Math.max(1, Math.floor(asNumber(ddns.ttl, defaults.ddns.ttl))),
    },
  };
}

function normalizeRuleDataSource(
  value: unknown,
  index: number,
): StandardRuleDataSource | null {
  const source = asRecord(value);
  const type = asString(source.type);
  const base = {
    id: cleanId(source.id, `source_${index + 1}`),
    name: asString(source.name, `Source ${index + 1}`).trim(),
    enabled: asBoolean(source.enabled, true),
  };
  if (type === "manual") {
    return { ...base, type, rules: asStringArray(source.rules) };
  }
  if (type === "local_file") {
    return { ...base, type, path: asString(source.path).trim() };
  }
  if (type === "subscription") {
    return {
      ...base,
      type,
      url: asString(source.url).trim(),
      updateIntervalHours: Math.max(
        1,
        asNumber(source.updateIntervalHours, 24),
      ),
      maxAgeHours: Math.max(1, asNumber(source.maxAgeHours, 72)),
    };
  }
  if (type === "native_dat") {
    return {
      ...base,
      type,
      path: asString(source.path).trim(),
      selectors: asStringArray(source.selectors),
    };
  }
  return null;
}

function normalizeRuleDataRole(value: unknown): StandardRuleDataRole {
  const source = asRecord(value);
  return {
    sources: Array.isArray(source.sources)
      ? source.sources
          .map(normalizeRuleDataSource)
          .filter((item): item is StandardRuleDataSource => item !== null)
      : [],
  };
}

function normalizeRuleData(value: unknown): StandardRuleDataSettings {
  const source = asRecord(value);
  return {
    domesticDomains: normalizeRuleDataRole(source.domesticDomains),
    foreignDomains: normalizeRuleDataRole(source.foreignDomains),
    domesticIps: normalizeRuleDataRole(source.domesticIps),
    directDomains: normalizeRuleDataRole(source.directDomains),
    remoteDomains: normalizeRuleDataRole(source.remoteDomains),
    ddnsDomains: normalizeRuleDataRole(source.ddnsDomains),
  };
}

function normalizeSmartRouting(value: unknown): StandardSmartRoutingSettings {
  const defaults = createDefaultStandardSettings().smartRouting;
  const source = asRecord(value);
  const response = asRecord(source.responsePolicy);
  const unknownMode =
    source.unknownMode === "privacy_first" ||
    source.unknownMode === "strict_remote"
      ? source.unknownMode
      : "compatibility_first";
  return {
    enabled: asBoolean(source.enabled, false),
    ...(asString(source.domesticPathId).trim()
      ? { domesticPathId: asString(source.domesticPathId).trim() }
      : {}),
    ...(asString(source.remotePathId).trim()
      ? { remotePathId: asString(source.remotePathId).trim() }
      : {}),
    unknownMode,
    privacyFallbackToDomestic: asBoolean(
      source.privacyFallbackToDomestic,
      false,
    ),
    fallbackThresholdMs: Math.max(
      1,
      asNumber(source.fallbackThresholdMs, defaults.fallbackThresholdMs),
    ),
    responsePolicy: {
      domesticIpMismatch: asBoolean(response.domesticIpMismatch, true),
      cnameOnly: asBoolean(response.cnameOnly, true),
      nodata: asBoolean(response.nodata, true),
      nxdomain: asBoolean(response.nxdomain, true),
      servfail: asBoolean(response.servfail, true),
      timeout: asBoolean(response.timeout, true),
      transportFailure: asBoolean(response.transportFailure, true),
    },
  };
}

function normalizeSubscription(value: unknown): StandardSubscription | null {
  const source = asRecord(value);
  const url = asString(source.url).trim();
  if (!url) return null;
  const id = cleanId(source.id, `subscription_${hashString(url).slice(0, 8)}`);
  return {
    id,
    name: asString(source.name, id),
    url,
    enabled: asBoolean(source.enabled, true),
    updateIntervalHours: Math.max(1, asNumber(source.updateIntervalHours, 24)),
  };
}

function normalizeRouting(value: unknown): StandardRoutingSettings {
  const source = asRecord(value);
  return {
    enabled: asBoolean(source.enabled, false),
    rules: Array.isArray(source.rules)
      ? source.rules
          .map(normalizeRoutingRule)
          .filter((item): item is StandardRoutingRule => item !== null)
      : [],
    scenarios: Array.isArray(source.scenarios)
      ? source.scenarios
          .map(normalizeScenario)
          .filter((item): item is StandardScenario => item !== null)
      : [],
  };
}

function normalizeRoutingRule(
  value: unknown,
  index: number,
): StandardRoutingRule | null {
  const source = asRecord(value);
  const condition = normalizeRuleCondition(source.condition);
  const action = normalizeRuleAction(source.action);
  if (!condition || !action) return null;
  return {
    id: cleanId(source.id, `rule_${index + 1}`),
    name: asString(source.name, `Rule ${index + 1}`),
    enabled: asBoolean(source.enabled, true),
    condition,
    action,
    source:
      source.source === "scenario" || source.source === "subscription"
        ? source.source
        : "manual",
    ...(asString(source.note).trim()
      ? { note: asString(source.note).trim() }
      : {}),
  };
}

function normalizeRuleCondition(
  value: unknown,
): StandardRoutingRule["condition"] | null {
  const source = asRecord(value);
  if (source.type === "subscription") {
    const subscriptionId = cleanId(source.subscriptionId, "");
    return subscriptionId ? { type: "subscription", subscriptionId } : null;
  }
  if (
    source.type === "domain" ||
    source.type === "suffix" ||
    source.type === "keyword" ||
    source.type === "client_cidr" ||
    source.type === "client_name" ||
    source.type === "qtype"
  ) {
    const values = asStringArray(source.values);
    return values.length > 0 ? { type: source.type, values } : null;
  }
  return null;
}

function normalizeRuleAction(
  value: unknown,
): StandardRoutingRule["action"] | null {
  const source = asRecord(value);
  if (source.type === "use_path") {
    return { type: "use_path", pathId: asString(source.pathId).trim() };
  }
  if (
    source.type === "use_default_path" ||
    source.type === "block" ||
    source.type === "allow" ||
    source.type === "skip_filtering" ||
    source.type === "prefer_ipv4" ||
    source.type === "prefer_ipv6" ||
    source.type === "disable_logging"
  ) {
    return { type: source.type };
  }
  return null;
}

function normalizeScenario(
  value: unknown,
  index: number,
): StandardScenario | null {
  const source = asRecord(value);
  if (
    source.kind !== "privacy" &&
    source.kind !== "gaming" &&
    source.kind !== "child_protection" &&
    source.kind !== "domestic_optimization"
  ) {
    return null;
  }
  return {
    id: cleanId(source.id, `scenario_${index + 1}`),
    name: asString(source.name, `Scenario ${index + 1}`),
    enabled: asBoolean(source.enabled, true),
    kind: source.kind,
  };
}

function normalizeException(
  value: unknown,
  index: number,
): StandardExceptionRule | null {
  const source = asRecord(value);
  const condition = normalizeRuleCondition(source.condition);
  const action = normalizeRuleAction(source.action);
  if (!condition || !action) return null;
  return {
    id: cleanId(source.id, `exception_${index + 1}`),
    name: asString(source.name, `Exception ${index + 1}`),
    enabled: asBoolean(source.enabled, true),
    condition,
    action,
    ...(asString(source.note).trim()
      ? { note: asString(source.note).trim() }
      : {}),
  };
}

function normalizeDevice(
  value: unknown,
  index: number,
): StandardDeviceProfile | null {
  const source = asRecord(value);
  const addresses = asStringArray(source.addresses);
  if (addresses.length === 0) return null;
  const filtering =
    source.filtering === "enabled" || source.filtering === "disabled"
      ? source.filtering
      : source.filtering === "inherit"
        ? "inherit"
        : undefined;
  const queryLog =
    source.queryLog === "enabled" || source.queryLog === "disabled"
      ? source.queryLog
      : source.queryLog === "inherit"
        ? "inherit"
        : undefined;
  return {
    id: cleanId(source.id, `device_${index + 1}`),
    name: asString(source.name, `Device ${index + 1}`),
    addresses,
    ...(asString(source.assignedPathId).trim()
      ? { assignedPathId: asString(source.assignedPathId).trim() }
      : {}),
    ...(filtering ? { filtering } : {}),
    ...(queryLog ? { queryLog } : {}),
  };
}

function normalizeSystem(value: unknown): StandardSystemSettings {
  const defaults = createDefaultStandardSettings().system;
  const source = asRecord(value);
  const logLevel =
    source.logLevel === "trace" ||
    source.logLevel === "debug" ||
    source.logLevel === "warn" ||
    source.logLevel === "error"
      ? source.logLevel
      : defaults.logLevel;
  const threads = asNumber(source.threads, NaN);
  return {
    logLevel,
    ...(Number.isFinite(threads) && threads > 0
      ? { threads: Math.floor(threads) }
      : {}),
  };
}

export function normalizeStandardSettings(
  value: unknown,
): StandardSettingsLoadResult {
  const source = asRecord(value);
  if (source.schema === 1) {
    return {
      settings: migrateLegacyStandardSettings(source),
      notice: "legacy_migrated",
    };
  }
  if (
    source.schema !== 2 &&
    source.schema !== 3 &&
    source.schema !== 4 &&
    source.schema !== 5
  ) {
    return {
      settings: createDefaultStandardSettings(),
      notice: "invalid_fallback",
    };
  }

  const upstreamGroups = Array.isArray(source.upstreamGroups)
    ? source.upstreamGroups
        .map(normalizeUpstreamGroup)
        .filter((item): item is StandardUpstreamGroup => item !== null)
    : [];
  const paths = Array.isArray(source.paths)
    ? source.paths
        .map(normalizePath)
        .filter((item): item is StandardResolutionPath => item !== null)
    : [];
  if (upstreamGroups.length === 0 || paths.length === 0) {
    return {
      settings: createDefaultStandardSettings(),
      notice: "invalid_fallback",
    };
  }

  const migratedFromV2 = source.schema === 2;
  const migratedFromV3 = source.schema === 3;
  const migratedFromV4 = source.schema === 4;
  const defaultGroupIndex = migratedFromV2
    ? Math.max(
        0,
        upstreamGroups.findIndex(
          (group) => group.isDefault || group.id === "default",
        ),
      )
    : -1;
  return {
    settings: {
      schema: 5,
      listen: {
        address: asString(asRecord(source.listen).address, "0.0.0.0:5335"),
        udp: asBoolean(asRecord(source.listen).udp, true),
        tcp: asBoolean(asRecord(source.listen).tcp, true),
      },
      upstreamGroups: migratedFromV2
        ? upstreamGroups.map((group, index) => ({
            ...group,
            isDefault: index === defaultGroupIndex,
          }))
        : upstreamGroups,
      paths: migratedFromV2
        ? paths.map((path) => ({
            ...path,
            dualStack: "inherit",
            ipSelection: createDefaultStandardSettings().paths[0].ipSelection,
            ecs: { mode: "inherit" as const },
          }))
        : paths,
      filtering: normalizeFiltering(source.filtering),
      local: normalizeLocal(source.local),
      ruleData: normalizeRuleData(source.ruleData),
      smartRouting: normalizeSmartRouting(source.smartRouting),
      cache: normalizeCache(source.cache),
      queryLog: migratedFromV2
        ? { ...normalizeQueryLog(source.queryLog), sampleRate: 1 }
        : normalizeQueryLog(source.queryLog),
      routing: migratedFromV2
        ? { ...normalizeRouting(source.routing), scenarios: [] }
        : normalizeRouting(source.routing),
      exceptions: Array.isArray(source.exceptions)
        ? source.exceptions
            .map(normalizeException)
            .filter((item): item is StandardExceptionRule => item !== null)
        : [],
      devices: Array.isArray(source.devices)
        ? source.devices
            .map(normalizeDevice)
            .filter((item): item is StandardDeviceProfile => item !== null)
        : [],
      system: normalizeSystem(source.system),
    },
    notice:
      migratedFromV2 || migratedFromV3 || migratedFromV4
        ? "legacy_migrated"
        : null,
  };
}

export function migrateLegacyStandardSettings(
  value: unknown,
): StandardModeSettings {
  const defaults = createDefaultStandardSettings();
  const source = asRecord(value);
  const legacyUpstreams = Array.isArray(source.upstreams)
    ? source.upstreams
        .map((item, index) => normalizeLegacyUpstream(item, index, "global"))
        .filter((item): item is StandardUpstream => item !== null)
    : defaults.upstreamGroups[0].upstreams;
  const split = asRecord(source.split);
  const domesticUpstreams = Array.isArray(split.domesticUpstreams)
    ? split.domesticUpstreams
        .map((item, index) => normalizeLegacyUpstream(item, index, "domestic"))
        .filter((item): item is StandardUpstream => item !== null)
    : [];

  const upstreamGroups: StandardUpstreamGroup[] = [
    {
      ...defaults.upstreamGroups[0],
      upstreams:
        legacyUpstreams.length > 0
          ? legacyUpstreams
          : defaults.upstreamGroups[0].upstreams,
    },
  ];
  const paths: StandardResolutionPath[] = [defaults.paths[0]];
  if (domesticUpstreams.length > 0) {
    upstreamGroups.push({
      id: "domestic",
      name: "Domestic upstream group",
      strategy: "balanced",
      upstreams: domesticUpstreams,
    });
    paths.push({
      ...defaults.paths[0],
      id: "domestic",
      name: "Domestic path",
      upstreamGroupId: "domestic",
    });
  }

  const adBlock = asRecord(source.adBlock);
  const legacyFiltering: StandardFilteringSettings = {
    ...defaults.filtering,
    enabled: asBoolean(adBlock.enabled, defaults.filtering.enabled),
    blockRules: asStringArray(adBlock.inlineRules),
  };

  return {
    ...defaults,
    listen: {
      address: asString(
        asRecord(source.listen).address,
        defaults.listen.address,
      ),
      udp: asBoolean(asRecord(source.listen).udp, defaults.listen.udp),
      tcp: asBoolean(asRecord(source.listen).tcp, defaults.listen.tcp),
    },
    upstreamGroups,
    paths,
    filtering: legacyFiltering,
    cache: normalizeCache(source.cache),
    queryLog: normalizeQueryLog(source.queryLog),
    system: normalizeSystem(source.system),
  };
}

function normalizeLegacyUpstream(
  value: unknown,
  index: number,
  group: "global" | "domestic",
): StandardUpstream | null {
  const source = asRecord(value);
  const address = asString(source.address ?? source.addr).trim();
  if (!address) return null;
  const id = cleanId(source.id ?? source.tag, `${group}_${index + 1}`);
  return {
    id,
    name: asString(source.name, id),
    protocol: "auto",
    address,
    enabled: asBoolean(source.enabled, true),
    ...(asString(source.bootstrap).trim()
      ? { bootstrap: asString(source.bootstrap).trim() }
      : {}),
    tlsVerify: true,
  };
}

function hashString(value: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < value.length; i += 1) {
    hash ^= value.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return `fnv1a32:${(hash >>> 0).toString(16).padStart(8, "0")}`;
}
