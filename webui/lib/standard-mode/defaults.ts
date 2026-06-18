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
    strategy: "parallel",
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
    ipSelection: "inherit",
    ecs: "inherit",
  };
}

export function createDefaultStandardSettings(): StandardModeSettings {
  return {
    schema: 2,
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
      blockRules: [],
      allowRules: [],
      blockResponse: "null_ip",
    },
    cache: {
      enabled: true,
      size: 8192,
      minTtl: 60,
      maxTtl: 86400,
      negativeTtl: 300,
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
