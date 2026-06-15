import type { OxiDnsConfig } from "../oxidns-config";

export interface StandardUpstream {
  id: string;
  name: string;
  address: string;
  bootstrap?: string;
  enabled: boolean;
  group: "global" | "domestic";
}

export interface StandardListenSettings {
  address: string;
  udp: boolean;
  tcp: boolean;
}

export interface StandardCacheSettings {
  enabled: boolean;
  size: number;
  minTtl: number;
  maxTtl: number;
  negativeTtl: number;
}

export interface StandardQueryLogSettings {
  enabled: boolean;
  retentionDays: number;
  sampleRate: number;
}

export interface StandardAdBlockSettings {
  enabled: boolean;
  inlineRules: string[];
}

export interface StandardSplitSettings {
  enabled: boolean;
  domesticDomains: string[];
  domesticUpstreams: StandardUpstream[];
}

export interface StandardDualStackSettings {
  strategy: "auto" | "prefer_ipv4" | "prefer_ipv6" | "ipv4_only" | "ipv6_only";
}

export interface StandardIpSelectionSettings {
  enabled: boolean;
  strategy: "lowest_latency" | "prefer_cn";
}

export interface StandardEcsSettings {
  enabled: boolean;
  ipv4Prefix?: string;
  ipv6Prefix?: string;
}

export interface StandardSystemSettings {
  logLevel: "trace" | "debug" | "info" | "warn" | "error";
  threads?: number;
}

export interface StandardModeSettings {
  schema: 1;
  listen: StandardListenSettings;
  upstreams: StandardUpstream[];
  cache: StandardCacheSettings;
  queryLog: StandardQueryLogSettings;
  adBlock: StandardAdBlockSettings;
  split: StandardSplitSettings;
  dualStack: StandardDualStackSettings;
  ipSelection: StandardIpSelectionSettings;
  ecs: StandardEcsSettings;
  system: StandardSystemSettings;
}

export interface StandardGenerationResult {
  config: OxiDnsConfig;
  skippedCapabilities: string[];
  generatedTags: string[];
}
