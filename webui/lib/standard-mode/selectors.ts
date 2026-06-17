import { isPluginKindSupported } from "../build-capabilities";
import type { BuildInfo } from "../oxidns-api";
import type { OxiDnsConfig } from "../oxidns-config";
import type {
  StandardModeSettings,
  StandardUpstream,
  StandardUpstreamGroup,
} from "./types";

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
