import { isPluginKindSupported } from "../build-capabilities";
import { createServerSettings } from "./defaults";
import type { BuildInfo } from "../oxidns-api";
import type {
  StandardDeviceProfile,
  StandardExceptionRule,
  StandardFilteringSettings,
  StandardModeSettings,
  StandardResolutionPath,
  StandardRoutingRule,
  StandardRoutingSettings,
  StandardServerProtocol,
  StandardServerSettings,
  StandardSubscription,
  StandardUpstream,
  StandardUpstreamProtocol,
} from "./types";

export interface StandardDnsValidationIssue {
  field: string;
  code:
    | "listen_required"
    | "server_listen_required"
    | "server_tls_required"
    | "server_protocol_unsupported"
    | "server_port_conflict"
    | "doh_path_required"
    | "upstream_required"
    | "upstream_address_required"
    | "protocol_unsupported";
  protocol?: StandardUpstreamProtocol;
  serverProtocol?: StandardServerProtocol;
  conflictWith?: string;
  requiredFeatures?: string[];
}

export interface StandardFilteringCapabilityMap {
  adRules: boolean;
  blackHole: boolean;
  download: boolean;
  cron: boolean;
  reloadProvider: boolean;
  subscriptionRuntime: boolean;
}

export interface StandardFilteringValidationIssue {
  field: string;
  code:
    | "capability_required"
    | "rule_source_required"
    | "subscription_runtime_required"
    | "subscription_name_required"
    | "subscription_url_required"
    | "subscription_url_invalid"
    | "subscription_interval_invalid";
  subscriptionId?: string;
}

export interface StandardRoutingCapabilityMap {
  sequence: boolean;
  qname: boolean;
  clientIp: boolean;
  qtype: boolean;
}

export interface StandardRoutingValidationIssue {
  field: string;
  code:
    | "capability_required"
    | "path_required"
    | "path_name_required"
    | "path_upstream_group_required"
    | "path_delete_blocked"
    | "rule_name_required"
    | "rule_condition_required"
    | "rule_action_required"
    | "rule_action_unsupported"
    | "rule_condition_unsupported"
    | "rule_matcher_unsupported";
  pathId?: string;
  ruleId?: string;
}

export interface StandardExceptionCapabilityMap extends StandardRoutingCapabilityMap {
  blackHole: boolean;
  preferIpv4: boolean;
  preferIpv6: boolean;
}

export interface StandardExceptionValidationIssue {
  field: string;
  code:
    | "capability_required"
    | "exception_name_required"
    | "exception_condition_required"
    | "exception_action_required"
    | "exception_action_unsupported"
    | "exception_condition_unsupported"
    | "exception_matcher_unsupported";
  exceptionId?: string;
}

export interface StandardDeviceCapabilityMap {
  sequence: boolean;
  clientIp: boolean;
  adRules: boolean;
  blackHole: boolean;
  queryRecorder: boolean;
}

export interface StandardDeviceValidationIssue {
  field: string;
  code:
    | "capability_required"
    | "device_name_required"
    | "device_address_required"
    | "device_address_invalid"
    | "device_path_required"
    | "filtering_capability_required"
    | "filtering_rule_source_required"
    | "filtering_subscription_runtime_required"
    | "query_log_capability_required";
  deviceId?: string;
}

const protocolFeatureRequirements: Record<
  StandardUpstreamProtocol,
  readonly string[]
> = {
  auto: [],
  udp: [],
  tcp: [],
  dot: ["upstream-dot"],
  doh: ["upstream-doh"],
  doh3: ["upstream-doh", "upstream-doh3"],
  doq: ["upstream-doq"],
};

export const STANDARD_UPSTREAM_PROTOCOLS: readonly StandardUpstreamProtocol[] = [
  "auto",
  "udp",
  "tcp",
  "dot",
  "doh",
  "doh3",
  "doq",
] as const;

export const STANDARD_SERVER_PROTOCOLS: readonly StandardServerProtocol[] = [
  "udp",
  "tcp",
  "dot",
  "doh",
  "doq",
] as const;

const serverProtocolFeatureRequirements: Record<
  StandardServerProtocol,
  readonly string[]
> = {
  udp: [],
  tcp: [],
  dot: ["server-dot"],
  doh: [],
  doq: [],
};

const serverProtocolKindRequirements: Record<
  StandardServerProtocol,
  "udp_server" | "tcp_server" | "http_server" | "quic_server"
> = {
  udp: "udp_server",
  tcp: "tcp_server",
  dot: "tcp_server",
  doh: "http_server",
  doq: "quic_server",
};

export function requiredStandardUpstreamProtocolFeatures(
  protocol: StandardUpstreamProtocol,
): readonly string[] {
  return protocolFeatureRequirements[protocol];
}

export function isStandardUpstreamProtocolSupported(
  protocol: StandardUpstreamProtocol,
  buildInfo: BuildInfo | null,
): boolean {
  const required = requiredStandardUpstreamProtocolFeatures(protocol);
  if (required.length === 0 || !buildInfo) return true;
  const enabled = new Set(buildInfo.enabled_features);
  return required.every((feature) => enabled.has(feature));
}

export function requiredStandardServerProtocolFeatures(
  protocol: StandardServerProtocol,
  settings?: StandardServerSettings,
): readonly string[] {
  if (protocol === "doh" && settings?.enableHttp3) {
    return ["server-doh3"];
  }
  return serverProtocolFeatureRequirements[protocol];
}

export function isStandardServerProtocolSupported(
  protocol: StandardServerProtocol,
  buildInfo: BuildInfo | null,
  settings?: StandardServerSettings,
): boolean {
  if (
    !isPluginKindSupported(
      buildInfo,
      "server",
      serverProtocolKindRequirements[protocol],
    )
  ) {
    return false;
  }
  const required = requiredStandardServerProtocolFeatures(protocol, settings);
  if (required.length === 0 || !buildInfo) return true;
  const enabled = new Set(buildInfo.enabled_features);
  return required.every((feature) => enabled.has(feature));
}

export function normalizeStandardDnsSettings(
  settings: StandardModeSettings,
): StandardModeSettings {
  const sampleRate = Number(settings.queryLog.sampleRate);
  const listenAddress = settings.listen.address.trim() || "0.0.0.0:5335";
  const servers = normalizeServerSettings(settings.listen.servers, listenAddress);
  return {
    ...settings,
    listen: {
      ...settings.listen,
      address: listenAddress,
      udp: servers.some((server) => server.protocol === "udp"),
      tcp: servers.some((server) => server.protocol === "tcp"),
      servers,
    },
    upstreamGroups: settings.upstreamGroups.map((group) => ({
      ...group,
      concurrent: Math.max(1, Math.min(3, Math.trunc(group.concurrent) || 1)),
      upstreams: group.upstreams.map(normalizeStandardUpstream),
    })),
    cache: {
      ...settings.cache,
      size: Math.max(128, Math.trunc(settings.cache.size) || 8192),
      minTtl: Math.max(0, Math.trunc(settings.cache.minTtl) || 0),
      maxTtl: Math.max(0, Math.trunc(settings.cache.maxTtl) || 0),
      negativeTtl: Math.max(0, Math.trunc(settings.cache.negativeTtl) || 0),
    },
    queryLog: {
      ...settings.queryLog,
      retentionDays: Math.max(
        1,
        Math.trunc(settings.queryLog.retentionDays) || 1,
      ),
      sampleRate: Number.isFinite(sampleRate)
        ? Math.min(1, Math.max(0, sampleRate))
        : 1,
    },
  };
}

function normalizeServerSettings(
  servers: StandardModeSettings["listen"]["servers"],
  listenAddress: string,
): StandardModeSettings["listen"]["servers"] {
  return servers.map((server, index) =>
    normalizeServer({
      ...createServerSettings(server.protocol, server.id || `server_${index + 1}`),
      ...server,
      listen: server.listen || (server.protocol === "udp" || server.protocol === "tcp"
        ? listenAddress
        : createServerSettings(server.protocol).listen),
    }),
  );
}

function normalizeServer(server: StandardServerSettings): StandardServerSettings {
  const idleTimeout = Number(server.idleTimeout);
  return {
    ...server,
    listen: server.listen.trim(),
    ...(server.cert !== undefined ? { cert: server.cert.trim() } : {}),
    ...(server.key !== undefined ? { key: server.key.trim() } : {}),
    ...(server.path !== undefined
      ? { path: server.path.trim() || "/dns-query" }
      : {}),
    ...(server.srcIpHeader !== undefined
      ? { srcIpHeader: server.srcIpHeader.trim() }
      : {}),
    ...(Number.isFinite(idleTimeout)
      ? { idleTimeout: Math.max(1, Math.trunc(idleTimeout)) }
      : {}),
  };
}

export function normalizeStandardUpstream(
  upstream: StandardUpstream,
): StandardUpstream {
  const address = upstream.address.trim();
  const dohPath = upstream.dohPath?.trim();
  const usesHttpDns =
    upstream.protocol === "doh" || upstream.protocol === "doh3";
  return {
    ...upstream,
    name: upstream.name.trim() || upstream.id,
    address,
    ...(upstream.bootstrap?.trim()
      ? { bootstrap: upstream.bootstrap.trim() }
      : { bootstrap: undefined }),
    ...(upstream.dialAddress?.trim()
      ? { dialAddress: upstream.dialAddress.trim() }
      : { dialAddress: undefined }),
    tlsVerify: upstream.tlsVerify ?? true,
    ...(usesHttpDns ? { dohPath: dohPath || "/dns-query" } : { dohPath: undefined }),
    enableHttp3: upstream.protocol === "doh3",
  };
}

export function validateStandardDnsSettings(
  settings: StandardModeSettings,
  buildInfo: BuildInfo | null,
): StandardDnsValidationIssue[] {
  const issues: StandardDnsValidationIssue[] = [];
  const servers = settings.listen.servers;
  if (servers.length === 0) {
    issues.push({ field: "listen", code: "listen_required" });
  }

  for (const server of servers) {
    const protocol = server.protocol;
    const field = `listen.${server.id}`;
    if (!server.listen.trim()) {
      issues.push({ field, code: "server_listen_required", serverProtocol: protocol });
    }
    if (
      (protocol === "dot" || protocol === "doq" || protocol === "doh") &&
      (!server.cert?.trim() || !server.key?.trim())
    ) {
      issues.push({ field, code: "server_tls_required", serverProtocol: protocol });
    }
    if (protocol === "doh" && !server.path?.trim()) {
      issues.push({ field, code: "doh_path_required", serverProtocol: protocol });
    }
    if (!isStandardServerProtocolSupported(protocol, buildInfo, server)) {
      issues.push({
        field,
        code: "server_protocol_unsupported",
        serverProtocol: protocol,
        requiredFeatures: [
          ...requiredStandardServerProtocolFeatures(protocol, server),
        ],
      });
    }
  }

  for (const issue of detectServerPortConflicts(servers)) {
    issues.push(issue);
  }

  const enabledUpstreams = settings.upstreamGroups.flatMap((group) =>
    group.upstreams
      .filter((item) => item.enabled)
      .map((upstream) => ({ groupId: group.id, upstream })),
  );
  const usableUpstreamCount = enabledUpstreams.filter(({ upstream }) =>
    upstream.address.trim(),
  ).length;

  if (usableUpstreamCount === 0) {
    issues.push({ field: "upstreams", code: "upstream_required" });
  }

  for (const { groupId, upstream } of enabledUpstreams) {
    const field = `upstream.${groupId}.${upstream.id}`;
    if (!upstream.address.trim()) {
      issues.push({ field, code: "upstream_address_required" });
    }
    if (!isStandardUpstreamProtocolSupported(upstream.protocol, buildInfo)) {
      issues.push({
        field,
        code: "protocol_unsupported",
        protocol: upstream.protocol,
        requiredFeatures: [...requiredStandardUpstreamProtocolFeatures(upstream.protocol)],
      });
    }
  }

  return issues;
}

function detectServerPortConflicts(
  servers: StandardServerSettings[],
): StandardDnsValidationIssue[] {
  const bindings: Array<{
    server: StandardServerSettings;
    transport: "udp" | "tcp";
    host: string;
    port: number;
  }> = [];
  for (const server of servers) {
    const endpoint = parseListenEndpoint(server.listen);
    if (!endpoint) continue;
    for (const transport of serverTransports(server)) {
      bindings.push({ server, transport, ...endpoint });
    }
  }

  const issues: StandardDnsValidationIssue[] = [];
  for (let leftIndex = 0; leftIndex < bindings.length; leftIndex += 1) {
    for (let rightIndex = leftIndex + 1; rightIndex < bindings.length; rightIndex += 1) {
      const left = bindings[leftIndex];
      const right = bindings[rightIndex];
      if (left.server.id === right.server.id) continue;
      if (left.transport !== right.transport || left.port !== right.port) continue;
      if (!hostsConflict(left.host, right.host)) continue;
      issues.push({
        field: `listen.${right.server.id}`,
        code: "server_port_conflict",
        serverProtocol: right.server.protocol,
        conflictWith: left.server.id,
      });
    }
  }
  return issues;
}

function serverTransports(server: StandardServerSettings): Array<"udp" | "tcp"> {
  if (server.protocol === "udp" || server.protocol === "doq") return ["udp"];
  if (server.protocol === "doh" && server.enableHttp3) return ["tcp", "udp"];
  return ["tcp"];
}

function parseListenEndpoint(listen: string): { host: string; port: number } | null {
  const value = listen.trim();
  if (!value) return null;
  const bracketMatch = /^\[([^\]]+)\]:(\d+)$/.exec(value);
  if (bracketMatch) {
    return { host: normalizeListenHost(bracketMatch[1]), port: Number(bracketMatch[2]) };
  }
  const portOnly = /^:(\d+)$/.exec(value);
  if (portOnly) return { host: "", port: Number(portOnly[1]) };
  const splitAt = value.lastIndexOf(":");
  if (splitAt < 0) return null;
  const port = Number(value.slice(splitAt + 1));
  if (!Number.isInteger(port) || port < 0 || port > 65535) return null;
  return { host: normalizeListenHost(value.slice(0, splitAt)), port };
}

function normalizeListenHost(host: string): string {
  return host.trim().replace(/^\[|\]$/g, "").toLowerCase();
}

function hostsConflict(left: string, right: string): boolean {
  return left === right || isWildcardHost(left) || isWildcardHost(right);
}

function isWildcardHost(host: string): boolean {
  return host === "" || host === "*" || host === "0.0.0.0" || host === "::";
}

export function standardFilteringCapabilityMap(
  buildInfo: BuildInfo | null,
): StandardFilteringCapabilityMap {
  const download = isPluginKindSupported(buildInfo, "executor", "download");
  const cron = isPluginKindSupported(buildInfo, "executor", "cron");
  const reloadProvider = isPluginKindSupported(
    buildInfo,
    "executor",
    "reload_provider",
  );
  return {
    adRules: isPluginKindSupported(buildInfo, "provider", "adguard_rule"),
    blackHole: isPluginKindSupported(buildInfo, "executor", "black_hole"),
    download,
    cron,
    reloadProvider,
    subscriptionRuntime: download && cron && reloadProvider,
  };
}

export function normalizeStandardFilteringSettings(
  settings: StandardModeSettings,
): StandardModeSettings {
  return {
    ...settings,
    filtering: normalizeFiltering(settings.filtering),
  };
}

export function normalizeAdGuardAllowRule(rule: string): string {
  const trimmed = rule.trim();
  if (!trimmed) return "";
  return trimmed.startsWith("@@") ? trimmed : `@@${trimmed}`;
}

function normalizeFiltering(
  filtering: StandardFilteringSettings,
): StandardFilteringSettings {
  return {
    ...filtering,
    subscriptions: filtering.subscriptions.map(normalizeSubscription),
    blockRules: uniqueLines(filtering.blockRules),
    allowRules: uniqueLines(filtering.allowRules).map(normalizeAdGuardAllowRule),
    blockResponse:
      filtering.blockResponse === "nxdomain" ||
      filtering.blockResponse === "refused"
        ? filtering.blockResponse
        : "null_ip",
  };
}

function normalizeSubscription(
  subscription: StandardSubscription,
): StandardSubscription {
  return {
    ...subscription,
    name: subscription.name.trim(),
    url: subscription.url.trim(),
    updateIntervalHours: Math.max(
      1,
      Math.trunc(subscription.updateIntervalHours) || 24,
    ),
  };
}

function uniqueLines(lines: string[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const line of lines) {
    const value = line.trim();
    if (!value || seen.has(value)) continue;
    seen.add(value);
    result.push(value);
  }
  return result;
}

export function validateStandardFilteringSettings(
  settings: StandardModeSettings,
  buildInfo: BuildInfo | null,
): StandardFilteringValidationIssue[] {
  const capabilities = standardFilteringCapabilityMap(buildInfo);
  const filtering = normalizeFiltering(settings.filtering);
  const issues: StandardFilteringValidationIssue[] = [];
  if (!filtering.enabled) return issues;

  if (!capabilities.adRules || !capabilities.blackHole) {
    issues.push({ field: "filtering", code: "capability_required" });
  }

  const enabledSubscriptions = filtering.subscriptions.filter(
    (subscription) => subscription.enabled,
  );
  if (filtering.blockRules.length === 0 && enabledSubscriptions.length === 0) {
    issues.push({ field: "filtering", code: "rule_source_required" });
  }

  if (
    filtering.blockRules.length === 0 &&
    enabledSubscriptions.length > 0 &&
    !capabilities.subscriptionRuntime
  ) {
    issues.push({
      field: "subscriptions",
      code: "subscription_runtime_required",
    });
  }

  for (const subscription of enabledSubscriptions) {
    const field = `subscription.${subscription.id}`;
    if (!subscription.name.trim()) {
      issues.push({
        field,
        code: "subscription_name_required",
        subscriptionId: subscription.id,
      });
    }
    if (!subscription.url.trim()) {
      issues.push({
        field,
        code: "subscription_url_required",
        subscriptionId: subscription.id,
      });
    } else if (!isHttpUrl(subscription.url)) {
      issues.push({
        field,
        code: "subscription_url_invalid",
        subscriptionId: subscription.id,
      });
    }
    if (
      !Number.isFinite(subscription.updateIntervalHours) ||
      subscription.updateIntervalHours < 1
    ) {
      issues.push({
        field,
        code: "subscription_interval_invalid",
        subscriptionId: subscription.id,
      });
    }
  }

  return issues;
}

export function standardDeviceCapabilityMap(
  buildInfo: BuildInfo | null,
): StandardDeviceCapabilityMap {
  return {
    sequence: isPluginKindSupported(buildInfo, "executor", "sequence"),
    clientIp: isPluginKindSupported(buildInfo, "matcher", "client_ip"),
    adRules: isPluginKindSupported(buildInfo, "provider", "adguard_rule"),
    blackHole: isPluginKindSupported(buildInfo, "executor", "black_hole"),
    queryRecorder: isPluginKindSupported(buildInfo, "executor", "query_recorder"),
  };
}

export function normalizeStandardDeviceSettings(
  settings: StandardModeSettings,
): StandardModeSettings {
  const pathIds = new Set(settings.paths.map((path) => path.id));
  const defaultPathId = settings.paths[0]?.id ?? "default";
  return {
    ...settings,
    devices: settings.devices.map((device, index) =>
      normalizeDeviceProfile(device, index, pathIds, defaultPathId),
    ),
  };
}

export function validateStandardDeviceSettings(
  settings: StandardModeSettings,
  buildInfo: BuildInfo | null,
): StandardDeviceValidationIssue[] {
  const normalized = normalizeStandardDeviceSettings(settings);
  const capabilities = standardDeviceCapabilityMap(buildInfo);
  const filteringCapabilities = standardFilteringCapabilityMap(buildInfo);
  const pathIds = new Set(normalized.paths.map((path) => path.id));
  const issues: StandardDeviceValidationIssue[] = [];
  const activeDevices = normalized.devices.filter(deviceHasPolicy);
  const forcedFiltering = normalized.devices.some(
    (device) => device.filtering === "enabled",
  );
  const forcedQueryLog = normalized.devices.some(
    (device) => device.queryLog === "enabled",
  );

  if (activeDevices.length > 0 && (!capabilities.sequence || !capabilities.clientIp)) {
    issues.push({ field: "devices", code: "capability_required" });
  }

  if (forcedFiltering) {
    const filtering = normalizeFiltering(normalized.filtering);
    const enabledSubscriptions = filtering.subscriptions.filter(
      (subscription) => subscription.enabled,
    );
    if (!filteringCapabilities.adRules || !filteringCapabilities.blackHole) {
      issues.push({
        field: "devices.filtering",
        code: "filtering_capability_required",
      });
    }
    if (filtering.blockRules.length === 0 && enabledSubscriptions.length === 0) {
      issues.push({
        field: "devices.filtering",
        code: "filtering_rule_source_required",
      });
    }
    if (
      filtering.blockRules.length === 0 &&
      enabledSubscriptions.length > 0 &&
      !filteringCapabilities.subscriptionRuntime
    ) {
      issues.push({
        field: "devices.filtering",
        code: "filtering_subscription_runtime_required",
      });
    }
  }

  if (forcedQueryLog && !capabilities.queryRecorder) {
    issues.push({
      field: "devices.queryLog",
      code: "query_log_capability_required",
    });
  }

  for (const device of normalized.devices) {
    const field = `device.${device.id}`;
    if (!device.name.trim()) {
      issues.push({
        field,
        code: "device_name_required",
        deviceId: device.id,
      });
    }
    if (device.addresses.length === 0) {
      issues.push({
        field,
        code: "device_address_required",
        deviceId: device.id,
      });
    }
    if (device.addresses.some((address) => !isClientAddress(address))) {
      issues.push({
        field,
        code: "device_address_invalid",
        deviceId: device.id,
      });
    }
    if (device.assignedPathId && !pathIds.has(device.assignedPathId)) {
      issues.push({
        field,
        code: "device_path_required",
        deviceId: device.id,
      });
    }
  }

  return issues;
}

function isHttpUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

function normalizeDeviceProfile(
  device: StandardDeviceProfile,
  index: number,
  pathIds: Set<string>,
  defaultPathId: string,
): StandardDeviceProfile {
  const assignedPathId = device.assignedPathId?.trim();
  return {
    ...device,
    id: cleanId(device.id, `device_${index + 1}`),
    name: device.name.trim(),
    addresses: uniqueLines(device.addresses),
    ...(assignedPathId
      ? { assignedPathId: pathIds.has(assignedPathId) ? assignedPathId : defaultPathId }
      : { assignedPathId: undefined }),
    filtering: normalizePolicy(device.filtering),
    queryLog: normalizePolicy(device.queryLog),
  };
}

function normalizePolicy(
  policy: StandardDeviceProfile["filtering"],
): "inherit" | "enabled" | "disabled" {
  if (policy === "enabled" || policy === "disabled") return policy;
  return "inherit";
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

function isClientAddress(value: string): boolean {
  const trimmed = value.trim();
  if (!trimmed || /\s/.test(trimmed)) return false;
  const [address, prefix, extra] = trimmed.split("/");
  if (!address || extra !== undefined) return false;
  if (prefix !== undefined) {
    const parsed = Number(prefix);
    const maxPrefix = address.includes(":") ? 128 : 32;
    if (!Number.isInteger(parsed) || parsed < 0 || parsed > maxPrefix) return false;
  }
  if (address.includes(":")) return /^[0-9a-f:.]+$/i.test(address);
  const octets = address.split(".");
  return (
    octets.length === 4 &&
    octets.every((octet) => {
      if (!/^\d{1,3}$/.test(octet)) return false;
      const parsed = Number(octet);
      return parsed >= 0 && parsed <= 255;
    })
  );
}

export function standardRoutingCapabilityMap(
  buildInfo: BuildInfo | null,
): StandardRoutingCapabilityMap {
  return {
    sequence: isPluginKindSupported(buildInfo, "executor", "sequence"),
    qname: isPluginKindSupported(buildInfo, "matcher", "qname"),
    clientIp: isPluginKindSupported(buildInfo, "matcher", "client_ip"),
    qtype: isPluginKindSupported(buildInfo, "matcher", "qtype"),
  };
}

export function standardExceptionCapabilityMap(
  buildInfo: BuildInfo | null,
): StandardExceptionCapabilityMap {
  return {
    ...standardRoutingCapabilityMap(buildInfo),
    blackHole: isPluginKindSupported(buildInfo, "executor", "black_hole"),
    preferIpv4: isPluginKindSupported(buildInfo, "executor", "prefer_ipv4"),
    preferIpv6: isPluginKindSupported(buildInfo, "executor", "prefer_ipv6"),
  };
}

export function normalizeStandardRoutingSettings(
  settings: StandardModeSettings,
): StandardModeSettings {
  const fallbackPath: StandardResolutionPath = {
    id: "default",
    name: "Default path",
    upstreamGroupId: settings.upstreamGroups[0]?.id ?? "default",
    filtering: "inherit",
    cache: "inherit",
    queryLog: "inherit",
    dualStack: "inherit",
    ipSelection: "inherit",
    ecs: "inherit",
  };
  const paths = settings.paths.length > 0
    ? settings.paths.map(normalizePath)
    : [fallbackPath];
  const normalizedPaths = paths.map((path, index) => ({
    ...path,
    id: index === 0 ? "default" : path.id,
  }));
  const pathIds = new Set(normalizedPaths.map((path) => path.id));
  const defaultPathId = normalizedPaths[0]?.id ?? "default";
  return {
    ...settings,
    paths: normalizedPaths,
    routing: normalizeRouting(settings.routing, pathIds, defaultPathId),
  };
}

export function normalizeStandardExceptionSettings(
  settings: StandardModeSettings,
): StandardModeSettings {
  const pathIds = new Set(settings.paths.map((path) => path.id));
  const defaultPathId = settings.paths[0]?.id ?? "default";
  return {
    ...settings,
    exceptions: settings.exceptions.map((exception) =>
      normalizeExceptionRule(exception, pathIds, defaultPathId),
    ),
  };
}

export function validateStandardRoutingSettings(
  settings: StandardModeSettings,
  buildInfo: BuildInfo | null,
): StandardRoutingValidationIssue[] {
  const capabilities = standardRoutingCapabilityMap(buildInfo);
  const normalized = normalizeStandardRoutingSettings(settings);
  const issues: StandardRoutingValidationIssue[] = [];
  const pathIds = new Set(normalized.paths.map((path) => path.id));
  const groupIds = new Set(normalized.upstreamGroups.map((group) => group.id));

  if (normalized.paths.length === 0) {
    issues.push({ field: "paths", code: "path_required" });
  }
  for (const path of normalized.paths) {
    if (!path.name.trim()) {
      issues.push({
        field: `path.${path.id}.name`,
        code: "path_name_required",
        pathId: path.id,
      });
    }
    if (!groupIds.has(path.upstreamGroupId)) {
      issues.push({
        field: `path.${path.id}.upstreamGroupId`,
        code: "path_upstream_group_required",
        pathId: path.id,
      });
    }
  }

  if (!normalized.routing.enabled) return issues;

  if (!capabilities.sequence) {
    issues.push({ field: "routing", code: "capability_required" });
  }

  for (const rule of normalized.routing.rules.filter((item) => item.enabled)) {
    const field = `rule.${rule.id}`;
    if (!rule.name.trim()) {
      issues.push({
        field,
        code: "rule_name_required",
        ruleId: rule.id,
      });
    }
    if (!isSupportedRoutingCondition(rule.condition)) {
      issues.push({
        field,
        code: "rule_condition_unsupported",
        ruleId: rule.id,
      });
    } else if (rule.condition.values.length === 0) {
      issues.push({
        field,
        code: "rule_condition_required",
        ruleId: rule.id,
      });
    }
    if (!isSupportedRoutingAction(rule.action)) {
      issues.push({
        field,
        code: "rule_action_unsupported",
        ruleId: rule.id,
      });
    } else if (rule.action.type === "use_path" && !pathIds.has(rule.action.pathId)) {
      issues.push({
        field,
        code: "rule_action_required",
        ruleId: rule.id,
      });
    }
    if (!isRoutingConditionCapabilitySupported(rule, capabilities)) {
      issues.push({
        field,
        code: "rule_matcher_unsupported",
        ruleId: rule.id,
      });
    }
  }

  return issues;
}

export function validateStandardExceptionSettings(
  settings: StandardModeSettings,
  buildInfo: BuildInfo | null,
): StandardExceptionValidationIssue[] {
  const normalized = normalizeStandardExceptionSettings(
    normalizeStandardRoutingSettings(settings),
  );
  const capabilities = standardExceptionCapabilityMap(buildInfo);
  const pathIds = new Set(normalized.paths.map((path) => path.id));
  const issues: StandardExceptionValidationIssue[] = [];

  if (!capabilities.sequence && normalized.exceptions.some((item) => item.enabled)) {
    issues.push({ field: "exceptions", code: "capability_required" });
  }

  for (const exception of normalized.exceptions.filter((item) => item.enabled)) {
    const field = `exception.${exception.id}`;
    if (!exception.name.trim()) {
      issues.push({
        field,
        code: "exception_name_required",
        exceptionId: exception.id,
      });
    }
    if (!isSupportedStandardCondition(exception.condition)) {
      issues.push({
        field,
        code: "exception_condition_unsupported",
        exceptionId: exception.id,
      });
    } else if (exception.condition.values.length === 0) {
      issues.push({
        field,
        code: "exception_condition_required",
        exceptionId: exception.id,
      });
    }
    if (!isSupportedExceptionAction(exception.action)) {
      issues.push({
        field,
        code: "exception_action_unsupported",
        exceptionId: exception.id,
      });
    } else if (
      exception.action.type === "use_path" &&
      !pathIds.has(exception.action.pathId)
    ) {
      issues.push({
        field,
        code: "exception_action_required",
        exceptionId: exception.id,
      });
    }
    if (!isConditionCapabilitySupported(exception, capabilities)) {
      issues.push({
        field,
        code: "exception_matcher_unsupported",
        exceptionId: exception.id,
      });
    }
    if (!isExceptionActionCapabilitySupported(exception, capabilities)) {
      issues.push({
        field,
        code: "exception_action_unsupported",
        exceptionId: exception.id,
      });
    }
  }

  return issues;
}

export function isPathReferencedByRouting(
  pathId: string,
  routing: StandardRoutingSettings,
): boolean {
  return routing.rules.some(
    (rule) =>
      rule.enabled &&
      rule.action.type === "use_path" &&
      rule.action.pathId === pathId,
  );
}

function normalizePath(path: StandardResolutionPath): StandardResolutionPath {
  return {
    ...path,
    id: cleanId(path.id, "path"),
    name: path.name.trim() || path.id,
    ...(path.description?.trim()
      ? { description: path.description.trim() }
      : { description: undefined }),
    upstreamGroupId: cleanId(path.upstreamGroupId, "default"),
  };
}

function normalizeRouting(
  routing: StandardRoutingSettings,
  pathIds: Set<string>,
  defaultPathId: string,
): StandardRoutingSettings {
  return {
    ...routing,
    rules: routing.rules.map((rule) => normalizeRoutingRule(rule, pathIds, defaultPathId)),
    scenarios: routing.scenarios.map((scenario) => ({
      ...scenario,
      id: cleanId(scenario.id, "scenario"),
      name: scenario.name.trim() || scenario.id,
    })),
  };
}

function normalizeRoutingRule(
  rule: StandardRoutingRule,
  pathIds: Set<string>,
  defaultPathId: string,
): StandardRoutingRule {
  const condition = isSupportedStandardCondition(rule.condition)
    ? {
        ...rule.condition,
        values: uniqueLines(rule.condition.values).map((value) =>
          normalizeConditionValue(rule.condition.type, value),
        ),
      }
    : rule.condition;
  const action =
    rule.action.type === "use_path"
      ? {
          type: "use_path" as const,
          pathId: pathIds.has(rule.action.pathId)
            ? rule.action.pathId
            : defaultPathId,
        }
      : rule.action;
  return {
    ...rule,
    id: cleanId(rule.id, "rule"),
    name: rule.name.trim() || rule.id,
    condition,
    action,
    ...(rule.note?.trim() ? { note: rule.note.trim() } : { note: undefined }),
  };
}

function normalizeExceptionRule(
  exception: StandardExceptionRule,
  pathIds: Set<string>,
  defaultPathId: string,
): StandardExceptionRule {
  const condition = isSupportedStandardCondition(exception.condition)
    ? {
        ...exception.condition,
        values: uniqueLines(exception.condition.values).map((value) =>
          normalizeConditionValue(exception.condition.type, value),
        ),
      }
    : exception.condition;
  const action =
    exception.action.type === "use_path"
      ? {
          type: "use_path" as const,
          pathId: pathIds.has(exception.action.pathId)
            ? exception.action.pathId
            : defaultPathId,
        }
      : exception.action;
  return {
    ...exception,
    id: cleanId(exception.id, "exception"),
    name: exception.name.trim() || exception.id,
    condition,
    action,
    ...(exception.note?.trim()
      ? { note: exception.note.trim() }
      : { note: undefined }),
  };
}

function cleanId(value: string, fallback: string): string {
  const raw = value.trim();
  return raw
    ? raw
        .toLowerCase()
        .replace(/[^a-z0-9_-]+/g, "_")
        .replace(/^_+|_+$/g, "") || fallback
    : fallback;
}

function normalizeConditionValue(
  type: StandardRoutingRule["condition"]["type"],
  value: string,
): string {
  const trimmed = value.trim();
  if (type === "qtype") return trimmed.toUpperCase();
  if (type === "suffix") return trimmed.replace(/^\.+/, "");
  return trimmed;
}

function isSupportedStandardCondition(
  condition: StandardRoutingRule["condition"],
): condition is Extract<
  StandardRoutingRule["condition"],
  { type: "domain" | "suffix" | "keyword" | "client_cidr" | "qtype" }
> {
  return (
    condition.type === "domain" ||
    condition.type === "suffix" ||
    condition.type === "keyword" ||
    condition.type === "client_cidr" ||
    condition.type === "qtype"
  );
}

function isSupportedRoutingCondition(
  condition: StandardRoutingRule["condition"],
): condition is Extract<
  StandardRoutingRule["condition"],
  { type: "domain" | "suffix" | "keyword" | "client_cidr" | "qtype" }
> {
  return isSupportedStandardCondition(condition);
}

function isSupportedRoutingAction(
  action: StandardRoutingRule["action"],
): action is Extract<
  StandardRoutingRule["action"],
  { type: "use_path" | "use_default_path" }
> {
  return action.type === "use_path" || action.type === "use_default_path";
}

function isRoutingConditionCapabilitySupported(
  rule: StandardRoutingRule,
  capabilities: StandardRoutingCapabilityMap,
): boolean {
  return isConditionCapabilitySupported(rule, capabilities);
}

function isConditionCapabilitySupported(
  rule: Pick<StandardRoutingRule, "condition">,
  capabilities: StandardRoutingCapabilityMap,
): boolean {
  if (!isSupportedStandardCondition(rule.condition)) return true;
  if (
    rule.condition.type === "domain" ||
    rule.condition.type === "suffix" ||
    rule.condition.type === "keyword"
  ) {
    return capabilities.qname;
  }
  if (rule.condition.type === "client_cidr") return capabilities.clientIp;
  if (rule.condition.type === "qtype") return capabilities.qtype;
  return true;
}

function isSupportedExceptionAction(
  action: StandardExceptionRule["action"],
): action is StandardExceptionRule["action"] {
  return (
    action.type === "use_path" ||
    action.type === "use_default_path" ||
    action.type === "block" ||
    action.type === "allow" ||
    action.type === "skip_filtering" ||
    action.type === "prefer_ipv4" ||
    action.type === "prefer_ipv6" ||
    action.type === "disable_logging"
  );
}

function isExceptionActionCapabilitySupported(
  exception: StandardExceptionRule,
  capabilities: StandardExceptionCapabilityMap,
): boolean {
  if (exception.action.type === "block") return capabilities.blackHole;
  if (exception.action.type === "prefer_ipv4") return capabilities.preferIpv4;
  if (exception.action.type === "prefer_ipv6") return capabilities.preferIpv6;
  return true;
}
