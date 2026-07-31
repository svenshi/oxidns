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
  strategy:
    | "fastest"
    | "balanced"
    | "prefer_positive"
    | "consensus"
    | "ordered_fallback";
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
  bootstrapVersion?: 4 | 6;
  dialAddress?: string;
  outbound?: string;
  socks5?: string;
  timeoutSeconds?: number;
  idleTimeoutSeconds?: number;
  maxConns?: number;
  minConns?: number;
  enablePipeline?: boolean;
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
  minPositiveTtl: number;
  maxPositiveTtl: number;
  maxNegativeTtl: number;
  negativeTtlWithoutSoa: number;
}

export interface StandardQueryLogSettings {
  enabled: boolean;
  retentionDays: number;
  sampleRate: number;
}

export interface StandardFilteringSettings {
  enabled: boolean;
  subscriptions: StandardSubscription[];
  localFiles: StandardFilterFile[];
  blockRules: string[];
  allowRules: string[];
  blockResponse: StandardBlockResponse;
}

export type StandardBlockResponse =
  | "null_ip"
  | "nxdomain"
  | "nodata"
  | "refused";

export interface StandardSubscription {
  id: string;
  name: string;
  url: string;
  enabled: boolean;
  updateIntervalHours: number;
}

export interface StandardFilterFile {
  id: string;
  name: string;
  path: string;
  enabled: boolean;
}

export interface StandardLocalSettings {
  hosts: {
    entries: string[];
    files: string[];
  };
  redirects: {
    rules: string[];
    files: string[];
  };
  records: {
    rules: string[];
    files: string[];
  };
  responseTtl: {
    enabled: boolean;
    min?: number;
    max?: number;
  };
  qtypePolicy: {
    enabled: boolean;
    qtypes: string[];
    response: StandardBlockResponse;
  };
  ddns: {
    enabled: boolean;
    domains: string[];
    pathId?: string;
    ttl: number;
  };
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
  schema: 4;
  listen: StandardListenSettings;
  upstreamGroups: StandardUpstreamGroup[];
  paths: StandardResolutionPath[];
  filtering: StandardFilteringSettings;
  local: StandardLocalSettings;
  cache: StandardCacheSettings;
  queryLog: StandardQueryLogSettings;
  routing: StandardRoutingSettings;
  exceptions: StandardExceptionRule[];
  devices: StandardDeviceProfile[];
  system: StandardSystemSettings;
}

export interface StandardTagMap {
  system: string[];
  caches?: Record<string, string>;
  /** Legacy frontend-generated metadata, readable during the v2 transition. */
  cache?: string;
  queryLog?: string;
  filtering?: string[];
  filterSubscriptions?: Record<string, StandardSubscriptionTagMap>;
  local?: Record<string, string>;
  upstreamGroups: Record<string, string>;
  paths: Record<string, string>;
  routingRules: Record<string, string>;
  exceptionRules: Record<string, string>;
  devices?: Record<string, string>;
}

export interface StandardSubscriptionTagMap {
  download: string;
  cron: string;
  job: string;
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
  localPolicyCount: number;
}

export interface StandardGeneratedMetadata {
  configVersion: string | null;
  settingsRevision: string;
  generatedTags: string[];
  tagMap: StandardTagMap;
  summary: StandardGenerationSummary;
  generatedAtMs: number;
  transactionId?: string;
}

export type StandardDiagnosticSeverity = "error" | "warning" | "suggestion";

export interface StandardDiagnostic {
  severity: StandardDiagnosticSeverity;
  code: string;
  path: string;
  message: string;
}

export type StandardOwnership = "managed" | "modified" | "unmanaged";

export interface StandardSemanticDiff {
  preserved_top_level: string[];
  generated_plugin_tags: string[];
  replaced_plugin_tags: string[];
  removed_plugin_tags: string[];
}

export interface StandardApplyBlocker {
  code: string;
  path: string;
  message: string;
}

export interface StandardGeneratedPlan {
  yaml: string;
  configVersion: string;
  pluginCount: number;
  generatedTags: string[];
  tagMap: StandardTagMap;
  summary: StandardGenerationSummary;
}

export interface StandardPolicyPlan {
  normalizedIntent: StandardModeSettings;
  diagnostics: StandardDiagnostic[];
  generated?: StandardGeneratedPlan;
  canApply: boolean;
  migration?: {
    from_schema: number;
    to_schema: number;
    diagnostics: StandardDiagnostic[];
  };
  details: Record<string, unknown>;
}

export interface StandardPlanResponse {
  ok: boolean;
  config_version: string;
  standard_version: string;
  ownership: StandardOwnership;
  semantic_diff: StandardSemanticDiff;
  blockers: StandardApplyBlocker[];
  can_apply: boolean;
  plan: StandardPolicyPlan;
}

export type StandardTransactionStatus =
  | "pending"
  | "succeeded"
  | "failed"
  | "recovered";

export interface StandardApplyResponse {
  ok: boolean;
  transaction_id: string;
  status: StandardTransactionStatus;
  target_config_version: string;
}

export interface StandardTransactionRecord {
  schema: number;
  transaction_id: string;
  status: StandardTransactionStatus;
  completed_at_ms: number;
  previous_config_version: string;
  candidate_config_version: string;
  error?: string;
}

export interface StandardTransactionStatusResponse {
  ok: boolean;
  transaction?: StandardTransactionRecord;
}
