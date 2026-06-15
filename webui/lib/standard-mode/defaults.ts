import type { StandardModeSettings } from "./types";

function upstream(
  id: string,
  name: string,
  address: string,
  group: "global" | "domestic" = "global",
) {
  return { id, name, address, enabled: true, group };
}

export function createDefaultStandardSettings(): StandardModeSettings {
  return {
    schema: 1,
    listen: {
      address: "0.0.0.0:5335",
      udp: true,
      tcp: true,
    },
    upstreams: [
      upstream("alidns", "AliDNS", "223.5.5.5:53", "global"),
      upstream("cloudflare", "Cloudflare", "1.1.1.1:53", "global"),
    ],
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
    adBlock: {
      enabled: false,
      inlineRules: [],
    },
    split: {
      enabled: false,
      domesticDomains: [],
      domesticUpstreams: [],
    },
    dualStack: {
      strategy: "auto",
    },
    ipSelection: {
      enabled: false,
      strategy: "lowest_latency",
    },
    ecs: {
      enabled: false,
    },
    system: {
      logLevel: "info",
    },
  };
}
