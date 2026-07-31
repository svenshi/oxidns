import { isPluginKindSupported } from "../build-capabilities";
import type { BuildInfo } from "../oxidns-api";
import type { OxiDnsConfig } from "../oxidns-config";
import type {
  StandardModeSettings,
  StandardUpstream,
  StandardUpstreamGroup,
} from "./types";

export type StandardReferenceKind =
  | "path"
  | "routing_rule"
  | "exception"
  | "device"
  | "ddns";

export interface StandardEntityReference {
  kind: StandardReferenceKind;
  id: string;
  name: string;
  href: string;
  enabled: boolean;
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
    blackHole: isPluginKindSupported(buildInfo, "executor", "black_hole"),
    domainSet: isPluginKindSupported(buildInfo, "provider", "domain_set"),
    forward: isPluginKindSupported(buildInfo, "executor", "forward"),
    ipSelector: isPluginKindSupported(buildInfo, "executor", "ip_selector"),
    preferIpv4: isPluginKindSupported(buildInfo, "executor", "prefer_ipv4"),
    preferIpv6: isPluginKindSupported(buildInfo, "executor", "prefer_ipv6"),
    upgrade: isPluginKindSupported(buildInfo, "executor", "plugin_upgrade"),
  };
}

export function selectDefaultUpstreamGroup(
  settings: StandardModeSettings,
): StandardUpstreamGroup {
  return (
    settings.upstreamGroups.find((group) => group.isDefault) ??
    settings.upstreamGroups[0]
  );
}

export function selectDefaultUpstreams(
  settings: StandardModeSettings,
): StandardUpstream[] {
  return selectDefaultUpstreamGroup(settings).upstreams;
}

export function selectAllStandardUpstreams(
  settings: StandardModeSettings,
): StandardUpstream[] {
  return settings.upstreamGroups.flatMap((group) => group.upstreams);
}

export function selectStandardUpstreamGroupReferences(
  settings: StandardModeSettings,
  groupId: string,
): StandardEntityReference[] {
  return settings.paths
    .filter((path) => path.upstreamGroupId === groupId)
    .map((path) => ({
      kind: "path",
      id: path.id,
      name: path.name || path.id,
      href: `/standard/routing#path-${encodeURIComponent(path.id)}`,
      enabled: true,
    }));
}

export function selectStandardPathReferences(
  settings: StandardModeSettings,
  pathId: string,
): StandardEntityReference[] {
  const references: StandardEntityReference[] = [];

  for (const rule of settings.routing.rules) {
    if (rule.action.type === "use_path" && rule.action.pathId === pathId) {
      references.push({
        kind: "routing_rule",
        id: rule.id,
        name: rule.name || rule.id,
        href: `/standard/routing#rule-${encodeURIComponent(rule.id)}`,
        enabled: rule.enabled,
      });
    }
  }
  for (const exception of settings.exceptions) {
    if (
      exception.action.type === "use_path" &&
      exception.action.pathId === pathId
    ) {
      references.push({
        kind: "exception",
        id: exception.id,
        name: exception.name || exception.id,
        href: `/standard/exceptions#exception-${encodeURIComponent(exception.id)}`,
        enabled: exception.enabled,
      });
    }
  }
  for (const device of settings.devices) {
    if (device.assignedPathId === pathId) {
      references.push({
        kind: "device",
        id: device.id,
        name: device.name || device.id,
        href: `/standard/devices#device-${encodeURIComponent(device.id)}`,
        enabled: true,
      });
    }
  }
  if (settings.local.ddns.pathId === pathId) {
    references.push({
      kind: "ddns",
      id: "ddns",
      name: "DDNS",
      href: "/standard/local#ddns",
      enabled: settings.local.ddns.enabled,
    });
  }

  return references;
}

export function selectStandardSummary(
  config: OxiDnsConfig | null,
  settings: StandardModeSettings | null,
) {
  const standardPlugins = (config?.plugins ?? []).filter((plugin) =>
    plugin.tag.startsWith("standard_"),
  );
  const enabledUpstreams =
    settings?.upstreamGroups.reduce(
      (sum, group) =>
        sum + group.upstreams.filter((item) => item.enabled).length,
      0,
    ) ?? 0;
  return {
    standardPluginCount: standardPlugins.length,
    upstreamGroupCount: settings?.upstreamGroups.length ?? 0,
    upstreamCount: enabledUpstreams,
    pathCount: settings?.paths.length ?? 0,
    cacheEnabled: Boolean(settings?.cache.enabled),
    adBlockEnabled: Boolean(settings?.filtering.enabled),
    splitEnabled: Boolean(settings?.routing.enabled),
    queryLogEnabled: Boolean(settings?.queryLog.enabled),
  };
}
