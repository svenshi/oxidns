import type { OxiDnsConfig } from "../oxidns-config";

export type StandardUpstreamProtocol =
  | "auto"
  | "udp"
  | "tcp"
  | "dot"
  | "doh"
  | "doh3"
  | "doq";

export interface StandardListenSettings {
  address: string;
  udp: boolean;
  tcp: boolean;
}

export interface StandardUpstreamGroup {
  id: string;
  name: string;
  description?: string;
  strategy: "parallel" | "sequential" | "fastest";
  upstreams: StandardUpstream[];
  isDefault?: boolean;
}

export interface StandardUpstream {
  id: string;
  name: string;
  protocol: StandardUpstreamProtocol;
  address: string;
  enabled: boolean;
  bootstrap?: string;
  dialAddress?: string;
  tlsVerify?: boolean;
  dohPath?: string;
  enableHttp3?: boolean;
}

export interface StandardResolutionPath {
  id: string;
  name: string;
  description?: string;
  upstreamGroupId: string;
  filtering: "inherit" | "enabled" | "disabled";
  cache: "inherit" | "enabled" | "disabled";
  queryLog: "inherit" | "enabled" | "disabled";
  dualStack:
    | "inherit"
    | "disabled"
    | "prefer_ipv4"
    | "prefer_ipv6"
    | "ipv4_only"
    | "ipv6_only";
  ipSelection: "inherit" | "enabled" | "disabled";
  ecs: "inherit" | "enabled" | "disabled";
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

export interface StandardFilteringSettings {
  enabled: boolean;
  subscriptions: StandardSubscription[];
  blockRules: string[];
  allowRules: string[];
  blockResponse: "null_ip" | "nxdomain" | "refused";
}

export interface StandardSubscription {
  id: string;
  name: string;
  url: string;
  enabled: boolean;
  updateIntervalHours: number;
}

export interface StandardRoutingSettings {
  enabled: boolean;
  rules: StandardRoutingRule[];
  scenarios: StandardScenario[];
}

export interface StandardRoutingRule {
  id: string;
  name: string;
  enabled: boolean;
  condition: StandardRuleCondition;
  action: StandardRuleAction;
  source: "manual" | "scenario" | "subscription";
  note?: string;
}

export interface StandardScenario {
  id: string;
  name: string;
  enabled: boolean;
  kind: "privacy" | "gaming" | "child_protection" | "domestic_optimization";
}

export type StandardRuleCondition =
  | { type: "domain"; values: string[] }
  | { type: "suffix"; values: string[] }
  | { type: "keyword"; values: string[] }
  | { type: "client_cidr"; values: string[] }
  | { type: "client_name"; values: string[] }
  | { type: "qtype"; values: string[] }
  | { type: "subscription"; subscriptionId: string };

export type StandardRuleAction =
  | { type: "use_path"; pathId: string }
  | { type: "use_default_path" }
  | { type: "block" }
  | { type: "allow" }
  | { type: "skip_filtering" }
  | { type: "prefer_ipv4" }
  | { type: "prefer_ipv6" }
  | { type: "disable_logging" };

export interface StandardExceptionRule {
  id: string;
  name: string;
  enabled: boolean;
  condition: StandardRuleCondition;
  action: StandardRuleAction;
  note?: string;
}

export interface StandardDeviceProfile {
  id: string;
  name: string;
  addresses: string[];
  assignedPathId?: string;
  filtering?: "inherit" | "enabled" | "disabled";
  queryLog?: "inherit" | "enabled" | "disabled";
}

export interface StandardSystemSettings {
  logLevel: "trace" | "debug" | "info" | "warn" | "error";
  threads?: number;
}

export interface StandardModeSettings {
  schema: 2;
  listen: StandardListenSettings;
  upstreamGroups: StandardUpstreamGroup[];
  paths: StandardResolutionPath[];
  filtering: StandardFilteringSettings;
  cache: StandardCacheSettings;
  queryLog: StandardQueryLogSettings;
  routing: StandardRoutingSettings;
  exceptions: StandardExceptionRule[];
  devices: StandardDeviceProfile[];
  system: StandardSystemSettings;
}

export interface StandardTagMap {
  system: string[];
  cache?: string;
  queryLog?: string;
  filtering?: string[];
  upstreamGroups: Record<string, string>;
  paths: Record<string, string>;
  routingRules: Record<string, string>;
  exceptionRules: Record<string, string>;
}

export interface StandardGenerationSummary {
  upstreamGroupCount: number;
  pathCount: number;
  enabledUpstreamCount: number;
  filteringEnabled: boolean;
  cacheEnabled: boolean;
  queryLogEnabled: boolean;
  routingRuleCount: number;
  exceptionRuleCount: number;
  deviceCount: number;
}

export interface StandardGeneratedMetadata {
  configVersion: string | null;
  settingsRevision: string;
  generatedTags: string[];
  tagMap: StandardTagMap;
  summary: StandardGenerationSummary;
  generatedAtMs: number;
}

export interface StandardGenerationResult {
  config: OxiDnsConfig;
  skippedCapabilities: string[];
  generatedTags: string[];
  tagMap: StandardTagMap;
  summary: StandardGenerationSummary;
}
