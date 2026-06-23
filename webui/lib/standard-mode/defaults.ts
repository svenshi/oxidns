import type {
  StandardModeSettings,
  StandardResolutionPath,
  StandardServerSettings,
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
    concurrent: 2,
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

export function createDefaultServerSettings(): StandardServerSettings[] {
  return [
    {
      id: "udp",
      protocol: "udp",
      listen: "0.0.0.0:5335",
    },
    {
      id: "tcp",
      protocol: "tcp",
      listen: "0.0.0.0:5335",
      idleTimeout: 10,
    },
  ];
}

export function createServerSettings(
  protocol: StandardServerSettings["protocol"],
  id: string = protocol,
): StandardServerSettings {
  if (protocol === "udp") {
    return { id, protocol, listen: "0.0.0.0:5335" };
  }
  if (protocol === "tcp") {
    return { id, protocol, listen: "0.0.0.0:5335", idleTimeout: 10 };
  }
  if (protocol === "doh") {
    return {
      id,
      protocol,
      listen: "0.0.0.0:443",
      path: "/dns-query",
      srcIpHeader: "",
      cert: "",
      key: "",
      idleTimeout: 30,
      enableHttp3: false,
    };
  }
  return {
    id,
    protocol,
    listen: "0.0.0.0:853",
    cert: "",
    key: "",
    idleTimeout: 10,
  };
}

export function createDefaultStandardSettings(): StandardModeSettings {
  const servers = createDefaultServerSettings();
  return {
    schema: 2,
    listen: {
      address: "0.0.0.0:5335",
      udp: true,
      tcp: true,
      servers,
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
