import type {
  StandardModeSettings,
  StandardResolutionPath,
  StandardUpstream,
  StandardUpstreamGroup,
} from "./types";

function upstream(
  id: string,
  name: string,
  address: string,
  protocol: StandardUpstream["protocol"] = "auto",
): StandardUpstream {
  return { id, name, protocol, address, enabled: true, tlsVerify: true };
}

export function createDefaultUpstreamGroup(): StandardUpstreamGroup {
  return {
    id: "default",
    name: "Default upstream group",
    strategy: "balanced",
    upstreams: [
      upstream("alidns", "AliDNS", "223.5.5.5:53"),
      upstream("cloudflare", "Cloudflare", "1.1.1.1:53"),
    ],
    isDefault: true,
  };
}

export function createDefaultResolutionPath(): StandardResolutionPath {
  return {
    id: "default",
    name: "Default path",
    upstreamGroupId: "default",
    filtering: "inherit",
    cache: "inherit",
    queryLog: "inherit",
    dualStack: "inherit",
    ipSelection: {
      enabled: false,
      selectionMode: "first_success",
      probeMethods: ["tcp:443", "tcp:80"],
      probeStaggerMs: 200,
      probeTimeoutMs: 600,
      maxWaitMs: 1000,
      topN: 1,
      dnssecPolicy: "reorder_only",
      maxParallelProbes: 256,
      cacheEnabled: true,
      cacheSize: 4096,
      cacheTtlSeconds: 3600,
      failureTtlSeconds: 60,
    },
    ecs: { mode: "inherit" },
  };
}

export function createDefaultStandardSettings(): StandardModeSettings {
  return {
    schema: 5,
    listen: {
      address: "0.0.0.0:5335",
      udp: true,
      tcp: true,
    },
    upstreamGroups: [createDefaultUpstreamGroup()],
    paths: [createDefaultResolutionPath()],
    filtering: {
      enabled: false,
      subscriptions: [],
      localFiles: [],
      blockRules: [],
      allowRules: [],
      blockResponse: "null_ip",
    },
    local: {
      hosts: { entries: [], files: [] },
      redirects: { rules: [], files: [] },
      records: { rules: [], files: [] },
      responseTtl: { enabled: false, min: 30, max: 86400 },
      qtypePolicy: { enabled: false, qtypes: [], response: "nodata" },
      ddns: { enabled: false, domains: [], ttl: 30 },
    },
    ruleData: {
      domesticDomains: { sources: [] },
      foreignDomains: { sources: [] },
      domesticIps: { sources: [] },
      directDomains: { sources: [] },
      remoteDomains: { sources: [] },
      ddnsDomains: { sources: [] },
    },
    smartRouting: {
      enabled: false,
      unknownMode: "compatibility_first",
      privacyFallbackToDomestic: false,
      fallbackThresholdMs: 500,
      responsePolicy: {
        domesticIpMismatch: true,
        cnameOnly: true,
        nodata: true,
        nxdomain: true,
        servfail: true,
        timeout: true,
        transportFailure: true,
      },
    },
    cache: {
      enabled: true,
      size: 8192,
      minPositiveTtl: 60,
      maxPositiveTtl: 86400,
      maxNegativeTtl: 300,
      negativeTtlWithoutSoa: 300,
    },
    queryLog: {
      enabled: true,
      retentionDays: 7,
      sampleRate: 1,
    },
    routing: {
      enabled: false,
      rules: [],
      scenarios: [],
    },
    exceptions: [],
    devices: [],
    system: {
      logLevel: "info",
    },
  };
}
