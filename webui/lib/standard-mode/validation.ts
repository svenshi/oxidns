import { isPluginKindSupported } from "../build-capabilities";
import type { BuildInfo } from "../oxidns-api";
import type {
  StandardFilteringSettings,
  StandardModeSettings,
  StandardResolutionPath,
  StandardRoutingRule,
  StandardRoutingSettings,
  StandardSubscription,
  StandardUpstream,
  StandardUpstreamProtocol,
} from "./types";

export interface StandardDnsValidationIssue {
  field: string;
  code:
    | "listen_required"
    | "upstream_required"
    | "upstream_address_required"
    | "protocol_unsupported";
  protocol?: StandardUpstreamProtocol;
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

export function normalizeStandardDnsSettings(
  settings: StandardModeSettings,
): StandardModeSettings {
  const sampleRate = Number(settings.queryLog.sampleRate);
  return {
    ...settings,
    listen: {
      ...settings.listen,
      address: settings.listen.address.trim() || "0.0.0.0:5335",
    },
    upstreamGroups: settings.upstreamGroups.map((group) => ({
      ...group,
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
  if (!settings.listen.udp && !settings.listen.tcp) {
    issues.push({ field: "listen", code: "listen_required" });
  }

  const defaultGroup = settings.upstreamGroups.find((group) => group.isDefault)
    ?? settings.upstreamGroups[0];
  const enabledUpstreams = defaultGroup?.upstreams.filter((item) => item.enabled) ?? [];
  const usableUpstreamCount = enabledUpstreams.filter((item) =>
    item.address.trim(),
  ).length;

  if (usableUpstreamCount === 0) {
    issues.push({ field: "upstreams", code: "upstream_required" });
  }

  for (const upstream of enabledUpstreams) {
    const field = `upstream.${upstream.id}`;
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

function isHttpUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
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

export function normalizeStandardRoutingSettings(
  settings: StandardModeSettings,
): StandardModeSettings {
  const fallbackPath: StandardResolutionPath = {
    id: "default",
    name: "默认解析路径",
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
  const condition = isSupportedRoutingCondition(rule.condition)
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

function isSupportedRoutingCondition(
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
  if (!isSupportedRoutingCondition(rule.condition)) return true;
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
