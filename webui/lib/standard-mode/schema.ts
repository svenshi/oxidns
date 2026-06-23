import {
  createDefaultServerSettings,
  createServerSettings,
  createDefaultStandardSettings,
} from "./defaults";
import type {
  StandardCacheSettings,
  StandardDeviceProfile,
  StandardExceptionRule,
  StandardFilteringSettings,
  StandardGeneratedMetadata,
  StandardGenerationResult,
  StandardListenSettings,
  StandardModeSettings,
  StandardQueryLogSettings,
  StandardResolutionPath,
  StandardRoutingRule,
  StandardRoutingSettings,
  StandardScenario,
  StandardServerProtocol,
  StandardServerSettings,
  StandardSubscription,
  StandardSystemSettings,
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

const serverProtocols: readonly StandardServerProtocol[] = [
  "udp",
  "tcp",
  "dot",
  "doh",
  "doq",
] as const;

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

function clampConcurrent(value: unknown, fallback: number): number {
  return Math.max(1, Math.min(3, Math.trunc(asNumber(value, fallback))));
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

function uniqueById<T extends { id: string }>(items: T[]): T[] {
  const seen = new Set<string>();
  const rows: T[] = [];
  for (const item of items) {
    if (seen.has(item.id)) continue;
    seen.add(item.id);
    rows.push(item);
  }
  return rows;
}

function normalizeUpstream(value: unknown, index: number): StandardUpstream | null {
  const source = asRecord(value);
  const address = asString(source.address ?? source.addr).trim();
  if (!address) return null;
  const protocol = upstreamProtocols.has(source.protocol as StandardUpstreamProtocol)
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
    ...(asString(source.dialAddress ?? source.dial_addr).trim()
      ? { dialAddress: asString(source.dialAddress ?? source.dial_addr).trim() }
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
    ? uniqueById(
        source.upstreams
          .map((item, upstreamIndex) => normalizeUpstream(item, upstreamIndex))
          .filter((item): item is StandardUpstream => item !== null),
      )
    : [];
  if (upstreams.length === 0 && id !== "default") return null;
  const defaultConcurrent = Math.max(1, Math.min(3, upstreams.length || 1));
  const legacyConcurrent =
    source.strategy === "sequential" ? 1 : defaultConcurrent;
  const concurrent = clampConcurrent(source.concurrent, legacyConcurrent);
  return {
    id,
    name: asString(
      source.name,
      id === "default" ? defaults.upstreamGroups[0].name : id,
    ),
    ...(asString(source.description).trim()
      ? { description: asString(source.description).trim() }
      : {}),
    concurrent,
    upstreams: upstreams.length > 0 ? upstreams : defaults.upstreamGroups[0].upstreams,
    ...(source.isDefault === true || id === "default" ? { isDefault: true } : {}),
  };
}

function normalizePath(value: unknown, index: number): StandardResolutionPath | null {
  const defaults = createDefaultStandardSettings();
  const source = asRecord(value);
  const id = cleanId(source.id, index === 0 ? "default" : `path_${index + 1}`);
  const filtering =
    source.filtering === "enabled" || source.filtering === "disabled"
      ? source.filtering
      : "inherit";
  const cache =
    source.cache === "enabled" || source.cache === "disabled" ? source.cache : "inherit";
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
  const ipSelection =
    source.ipSelection === "enabled" || source.ipSelection === "disabled"
      ? source.ipSelection
      : "inherit";
  const ecs =
    source.ecs === "enabled" || source.ecs === "disabled" ? source.ecs : "inherit";
  return {
    id,
    name: asString(source.name, id === "default" ? defaults.paths[0].name : id),
    ...(asString(source.description).trim()
      ? { description: asString(source.description).trim() }
      : {}),
    upstreamGroupId: cleanId(source.upstreamGroupId, "default"),
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
    minTtl: Math.max(0, asNumber(source.minTtl, defaults.minTtl)),
    maxTtl: Math.max(0, asNumber(source.maxTtl, defaults.maxTtl)),
    negativeTtl: Math.max(0, asNumber(source.negativeTtl, defaults.negativeTtl)),
  };
}

function normalizeQueryLog(value: unknown): StandardQueryLogSettings {
  const defaults = createDefaultStandardSettings().queryLog;
  const source = asRecord(value);
  return {
    enabled: asBoolean(source.enabled, defaults.enabled),
    retentionDays: Math.max(1, asNumber(source.retentionDays, defaults.retentionDays)),
    sampleRate: Math.min(
      1,
      Math.max(0, asNumber(source.sampleRate, defaults.sampleRate)),
    ),
  };
}

function normalizeListen(value: unknown): StandardListenSettings {
  const defaults = createDefaultStandardSettings().listen;
  const source = asRecord(value);
  const address = asString(source.address, defaults.address).trim() || defaults.address;
  const udp = asBoolean(source.udp, defaults.udp);
  const tcp = asBoolean(source.tcp, defaults.tcp);
  const servers = normalizeServerList(source.servers, address, udp, tcp);
  return {
    address,
    udp: servers.some((server) => server.protocol === "udp"),
    tcp: servers.some((server) => server.protocol === "tcp"),
    servers,
  };
}

function normalizeServerList(
  value: unknown,
  address: string,
  udp: boolean,
  tcp: boolean,
): StandardServerSettings[] {
  const rows = Array.isArray(value)
    ? value
        .map((item, index) => normalizeServerSettings(item, `server_${index + 1}`))
        .filter((item): item is StandardServerSettings => item !== null)
    : normalizeServerRecord(value, address);
  const withLegacyFallback = rows.length > 0
    ? rows
    : [
        ...(udp
          ? [normalizeServerSettings({ protocol: "udp", listen: address }, "udp")]
          : []),
        ...(tcp
          ? [normalizeServerSettings({ protocol: "tcp", listen: address }, "tcp")]
          : []),
      ].filter((item): item is StandardServerSettings => item !== null);
  const fallback = withLegacyFallback.length > 0
    ? withLegacyFallback
    : createDefaultServerSettings();
  return uniqueById(fallback);
}

function normalizeServerRecord(
  value: unknown,
  address: string,
): StandardServerSettings[] {
  const source = asRecord(value);
  return serverProtocols
    .map((protocol) => {
      const raw = asRecord(source[protocol]);
      if (Object.keys(raw).length === 0) return null;
      if (raw.enabled === false) return null;
      return normalizeServerSettings(
        {
          ...raw,
          protocol,
          id: raw.id ?? protocol,
          listen: raw.listen ?? raw.address ?? address,
        },
        protocol,
      );
    })
    .filter((item): item is StandardServerSettings => item !== null);
}

function normalizeServerSettings(
  value: unknown,
  fallbackId: string,
): StandardServerSettings | null {
  const source = asRecord(value);
  const protocol = serverProtocols.includes(source.protocol as StandardServerProtocol)
    ? (source.protocol as StandardServerProtocol)
    : null;
  if (!protocol) return null;
  const fallback = createServerSettings(protocol, fallbackId);
  const id = cleanId(source.id, fallback.id);
  const listen =
    asString(source.listen ?? source.address, fallback.listen).trim() ||
    fallback.listen;
  const cert = asString(source.cert, fallback.cert ?? "").trim();
  const key = asString(source.key, fallback.key ?? "").trim();
  const path = asString(source.path, fallback.path ?? "/dns-query").trim();
  const srcIpHeader = asString(
    source.srcIpHeader ?? source.src_ip_header,
    fallback.srcIpHeader ?? "",
  ).trim();
  const hasIdleTimeout =
    source.idleTimeout !== undefined ||
    source.idle_timeout !== undefined ||
    fallback.idleTimeout !== undefined;
  const idleTimeout = hasIdleTimeout
    ? Math.max(
        1,
        Math.trunc(
          asNumber(
            source.idleTimeout ?? source.idle_timeout,
            fallback.idleTimeout ?? 1,
          ),
        ),
      )
    : undefined;
  return {
    id,
    protocol,
    listen,
    ...(cert ? { cert } : fallback.cert !== undefined ? { cert: "" } : {}),
    ...(key ? { key } : fallback.key !== undefined ? { key: "" } : {}),
    ...(idleTimeout !== undefined ? { idleTimeout } : {}),
    ...(path || fallback.path !== undefined ? { path: path || "/dns-query" } : {}),
    ...(srcIpHeader || fallback.srcIpHeader !== undefined
      ? { srcIpHeader }
      : {}),
    ...(source.enableHttp3 !== undefined || fallback.enableHttp3 !== undefined
      ? { enableHttp3: asBoolean(source.enableHttp3, fallback.enableHttp3 ?? false) }
      : {}),
  };
}

function normalizeFiltering(value: unknown): StandardFilteringSettings {
  const defaults = createDefaultStandardSettings().filtering;
  const source = asRecord(value);
  const blockResponse =
    source.blockResponse === "nxdomain" || source.blockResponse === "refused"
      ? source.blockResponse
      : "null_ip";
  return {
    enabled: asBoolean(source.enabled, defaults.enabled),
    subscriptions: Array.isArray(source.subscriptions)
      ? source.subscriptions
          .map(normalizeSubscription)
          .filter((item): item is StandardSubscription => item !== null)
      : [],
    blockRules: asStringArray(source.blockRules),
    allowRules: asStringArray(source.allowRules),
    blockResponse,
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

function normalizeRoutingRule(value: unknown, index: number): StandardRoutingRule | null {
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
    ...(asString(source.note).trim() ? { note: asString(source.note).trim() } : {}),
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

function normalizeRuleAction(value: unknown): StandardRoutingRule["action"] | null {
  const source = asRecord(value);
  if (source.type === "use_path") {
    return { type: "use_path", pathId: cleanId(source.pathId, "default") };
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

function normalizeScenario(value: unknown, index: number): StandardScenario | null {
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
    ...(asString(source.note).trim() ? { note: asString(source.note).trim() } : {}),
  };
}

function normalizeDevice(value: unknown, index: number): StandardDeviceProfile | null {
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
      ? { assignedPathId: cleanId(source.assignedPathId, "default") }
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

export function normalizeStandardSettings(value: unknown): StandardSettingsLoadResult {
  const source = asRecord(value);
  if (source.schema === 1) {
    return { settings: migrateLegacyStandardSettings(source), notice: "legacy_migrated" };
  }
  if (source.schema !== 2) {
    return { settings: createDefaultStandardSettings(), notice: "invalid_fallback" };
  }

  const upstreamGroups = Array.isArray(source.upstreamGroups)
    ? uniqueById(
        source.upstreamGroups
          .map(normalizeUpstreamGroup)
          .filter((item): item is StandardUpstreamGroup => item !== null),
      )
    : [];
  const paths = Array.isArray(source.paths)
    ? uniqueById(
        source.paths
          .map(normalizePath)
          .filter((item): item is StandardResolutionPath => item !== null),
      )
    : [];
  if (upstreamGroups.length === 0 || paths.length === 0) {
    return { settings: createDefaultStandardSettings(), notice: "invalid_fallback" };
  }

  const groupIds = new Set(upstreamGroups.map((item) => item.id));
  const normalizedPaths = paths.map((path) => ({
    ...path,
    upstreamGroupId: groupIds.has(path.upstreamGroupId)
      ? path.upstreamGroupId
      : upstreamGroups[0].id,
  }));
  return {
    settings: {
      schema: 2,
      listen: normalizeListen(source.listen),
      upstreamGroups: upstreamGroups.map((group, index) => ({
        ...group,
        isDefault: index === 0 || group.id === "default" ? true : group.isDefault,
      })),
      paths: normalizedPaths,
      filtering: normalizeFiltering(source.filtering),
      cache: normalizeCache(source.cache),
      queryLog: normalizeQueryLog(source.queryLog),
      routing: normalizeRouting(source.routing),
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
    notice: null,
  };
}

export function migrateLegacyStandardSettings(value: unknown): StandardModeSettings {
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
      upstreams: legacyUpstreams.length > 0 ? legacyUpstreams : defaults.upstreamGroups[0].upstreams,
    },
  ];
  const paths: StandardResolutionPath[] = [defaults.paths[0]];
  if (domesticUpstreams.length > 0) {
    upstreamGroups.push({
      id: "domestic",
      name: "Domestic upstream group",
      concurrent: Math.max(1, Math.min(3, domesticUpstreams.length)),
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
    listen: normalizeListen(source.listen),
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

export function computeStandardSettingsRevision(
  settings: StandardModeSettings,
): string {
  return hashString(stableStringify(settings));
}

export function buildStandardGeneratedMetadata(
  settings: StandardModeSettings,
  result: StandardGenerationResult,
  configVersion: string | null,
): StandardGeneratedMetadata {
  return {
    configVersion,
    settingsRevision: computeStandardSettingsRevision(settings),
    generatedTags: result.generatedTags,
    tagMap: result.tagMap,
    summary: result.summary,
    generatedAtMs: Date.now(),
  };
}

function stableStringify(value: unknown): string {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  const record = value as Record<string, unknown>;
  return `{${Object.keys(record)
    .sort()
    .map((key) => `${JSON.stringify(key)}:${stableStringify(record[key])}`)
    .join(",")}}`;
}

function hashString(value: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < value.length; i += 1) {
    hash ^= value.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return `fnv1a32:${(hash >>> 0).toString(16).padStart(8, "0")}`;
}
