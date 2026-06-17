import type { BuildInfo } from "../oxidns-api";
import type {
  StandardModeSettings,
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
