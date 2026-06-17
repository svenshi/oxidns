"use client";

import { useAuthStore } from "./auth-store";
import { WEBUI, tClient } from "./i18n";

export interface ConfigFileResponse {
  ok: boolean;
  path: string;
  format: "yaml";
  content: string;
  version: string;
  updated_at_ms?: number;
}

export interface SaveConfigOptions {
  content: string;
  baseVersion?: string | null;
  validate?: boolean;
  reload?: boolean;
}

export interface SaveConfigResponse {
  ok: boolean;
  path: string;
  format: "yaml";
  version: string;
  updated_at_ms?: number;
  plugin_count: number;
  init_order: string[];
  dependency_graph?: DependencyGraphReport;
  reload?: ReloadSnapshot;
  message: string;
}

export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

export type JsonObject = { [key: string]: JsonValue };

export interface WebUiConfigResponse {
  ok: boolean;
  path: string;
  config: JsonObject;
  version: string;
  updated_at_ms: number;
  defaulted: boolean;
  recovered: boolean;
  backup_path: string | null;
}

export interface WebUiOptionsResponse {
  ok: boolean;
  persistent: boolean;
  patch: boolean;
  reset: boolean;
  max_bytes: number;
  schema: number;
  path: string;
  default_config: JsonObject;
}

export interface SaveWebUiConfigOptions {
  config: JsonObject;
  baseVersion?: string | null;
}

export interface PatchWebUiConfigOptions {
  patch: JsonObject;
  baseVersion?: string | null;
}

export interface DeleteWebUiConfigOptions {
  baseVersion?: string | null;
}

export interface HealthResponse {
  status: string;
  version: string;
  build_bundle?: string;
  uptime_ms: number;
  checks: {
    api: string;
    plugin_init: string;
    server_startup: string;
  };
  plugins: {
    total: number;
    servers: number;
  };
}

export interface SupportedPlugins {
  servers: string[];
  executors: string[];
  matchers: string[];
  providers: string[];
}

export interface BuildInfo {
  version: string;
  bundle: string;
  enabled_bundles: string[];
  enabled_features: string[];
  supported_plugins: SupportedPlugins;
}

export interface BuildInfoResponse {
  ok: boolean;
  build: BuildInfo;
}

export interface ReloadSnapshot {
  status: string;
  pending: boolean;
  in_progress: boolean;
  last_started_ms?: number;
  last_completed_ms?: number;
  last_success_ms?: number;
  last_error?: string;
  /** SHA256 of the config the backend is actually running (authoritative). */
  running_version?: string;
  /** SHA256 of the config the most recent reload attempted to apply. */
  target_version?: string;
}

export interface ControlResponse {
  status: string;
  uptime_ms: number;
  config_path: string;
  shutdown_requested: boolean;
  reload: ReloadSnapshot;
}

export interface SystemResponse {
  ok: boolean;
  version: string;
  build?: BuildInfo;
  os: string;
  arch: string;
  uptime_ms: number;
  config_path: string;
  api_enabled: boolean;
  reload: ReloadSnapshot;
  process_cpu_percent?: number;
  process_memory_mb?: number;
  system_memory_total_mb?: number;
}

export interface DependencyGraphNode {
  tag: string;
  plugin_type: string;
  kind: string;
}

export interface DependencyGraphEdge {
  source_tag: string;
  field: string;
  target_tag: string;
  expected_kind: string;
  expected_plugin_type?: string;
}

export interface SequenceFlowExpression {
  field: string;
  raw: string;
  kind: "plugin" | "quick_setup" | "builtin" | "invalid";
  target_tag?: string;
  plugin_type?: string;
  param?: string;
  inverted: boolean;
  builtin?: string;
}

export interface SequenceFlowRule {
  index: number;
  matches: SequenceFlowExpression[];
  exec?: SequenceFlowExpression;
}

export interface SequenceFlowReport {
  tag: string;
  rules: SequenceFlowRule[];
}

export interface DependencyGraphReport {
  nodes: DependencyGraphNode[];
  edges: DependencyGraphEdge[];
  init_order: string[];
  sequence_flows?: SequenceFlowReport[];
}

export interface ConfigValidateResponse {
  ok: boolean;
  source: "file" | "body";
  path?: string;
  plugin_count: number;
  dependency_graph: DependencyGraphReport;
  message: string;
}

export interface ConfigDiagnostic {
  message: string;
  severity: "error" | "warning" | "info";
  line: number;
  column: number;
  end_line: number;
  end_column: number;
}

export class ConfigValidationError extends Error {
  diagnostics: string[];
  diagnosticDetails: ConfigDiagnostic[];

  constructor(
    message: string,
    diagnostics: string[] = [message],
    diagnosticDetails: ConfigDiagnostic[] = [],
  ) {
    super(message);
    this.name = "ConfigValidationError";
    this.diagnostics = diagnostics;
    this.diagnosticDetails = diagnosticDetails;
  }
}

export interface CacheEntryRow {
  id: string;
  domain: string;
  record_type: string;
  dns_class: string;
  rcode: string;
  answer_count: number;
  authority_count?: number;
  additional_count?: number;
  ttl: number;
  remaining_ttl: number;
  fresh: boolean;
  stale: boolean;
  cache_time_ms: number;
  expire_at_ms: number;
  last_access_ms: number;
  cache_time_unix_ms?: number;
  expire_at_unix_ms?: number;
  last_access_unix_ms?: number;
  do_bit: boolean;
  cd_bit: boolean;
  answers_json?: QueryRecordPayload[];
  authorities_json?: QueryRecordPayload[];
  additionals_json?: QueryRecordPayload[];
  signature_json?: QueryRecordPayload[];
  ecs_scope?: {
    family: number;
    source_prefix: number;
    scope_prefix: number;
    network_hex: string;
  };
}

export interface CacheEntriesResponse {
  ok: boolean;
  entries: CacheEntryRow[];
  next_cursor?: string;
  total_entries: number;
}

export interface QueryQuestion {
  name: string;
  qtype: string;
  qclass: string;
}

export interface QueryRecordPayload {
  name: string;
  class: string;
  ttl: number;
  rr_type: string;
  payload_kind: string;
  payload_text: string;
  payload: unknown;
}

export interface QueryRecorderStep {
  event_index: number;
  sequence_tag: string;
  node_index?: number;
  kind: string;
  tag?: string;
  outcome: string;
}

export interface QueryRecordRow {
  id: number;
  created_at_ms: number;
  elapsed_ms: number;
  request_id: number;
  client_ip: string;
  questions_json: QueryQuestion[];
  error?: string;
  has_response: boolean;
  rcode?: string;
  answer_count: number;
  authority_count: number;
  additional_count: number;
  answers_json: QueryRecordPayload[];
  authorities_json: QueryRecordPayload[];
  additionals_json: QueryRecordPayload[];
  signature_json: QueryRecordPayload[];
  [key: string]: unknown;
}

export interface QueryRecordDetail extends QueryRecordRow {
  steps: QueryRecorderStep[];
}

export interface QueryRecordsResponse {
  ok: boolean;
  next_cursor?: string;
  records: QueryRecordRow[];
}

export interface QueryRecordDetailResponse {
  ok: boolean;
  record: QueryRecordDetail;
}

export interface QueryRecorderClearResponse {
  ok: boolean;
  cleared_records: number;
}

export type QueryRecordStatusFilter =
  | "all"
  | "error"
  | "has_response"
  | "no_response";

export interface QueryRecordFilters {
  sinceMs?: number;
  untilMs?: number;
  qname?: string;
  qtype?: string;
  clientIp?: string;
  rcode?: string;
  status?: QueryRecordStatusFilter;
  matcherTag?: string;
}

export type QueryRecorderPluginStatsKind =
  | "all"
  | "matcher"
  | "executor"
  | "builtin";

export interface QueryRecorderPluginStatsRow {
  kind: string;
  tag?: string;
  checked: number;
  matched: number;
  executed: number;
  query_total: number;
  query_share: number;
}

export interface QueryRecorderPluginStatsResponse {
  ok: boolean;
  query_total: number;
  stats: QueryRecorderPluginStatsRow[];
}

export interface QueryRecorderTopRow {
  key: string;
  count: number;
  share: number;
}

export interface QueryRecorderTopResponse {
  ok: boolean;
  sample_size: number;
  rows: QueryRecorderTopRow[];
}

export interface QueryRecorderDistributionRow {
  key: string;
  count: number;
  share: number;
}

export interface QueryRecorderDistributionResponse {
  ok: boolean;
  sample_size: number;
  rows: QueryRecorderDistributionRow[];
}

export interface QueryRecorderLatencyHistogramBucket {
  lt_ms: number | null;
  count: number;
}

export interface QueryRecorderLatencySlowRow {
  qname: string;
  count: number;
  avg_ms: number;
  max_ms: number;
}

export interface QueryRecorderLatencySummary {
  ok: boolean;
  sample_size: number;
  avg_ms: number;
  p50_ms: number;
  p95_ms: number;
  p99_ms: number;
  max_ms: number;
  histogram: QueryRecorderLatencyHistogramBucket[];
  slow_top: QueryRecorderLatencySlowRow[];
}

export type QueryRecorderTimeseriesBucket = "minute" | "hour";

export interface QueryRecorderTimeseriesPoint {
  bucket_ms: number;
  total: number;
  error_count: number;
  no_response_count: number;
  avg_ms: number;
  p95_ms: number;
}

export interface QueryRecorderTimeseriesResponse {
  ok: boolean;
  sample_size: number;
  bucket_ms: number;
  points: QueryRecorderTimeseriesPoint[];
}

export async function fetchConfigFile(): Promise<ConfigFileResponse> {
  const response = await fetch(apiUrl("/config"), {
    method: "GET",
    headers: apiHeaders(),
  });
  return readJsonResponse<ConfigFileResponse>(response);
}

export async function fetchWebUiConfig(): Promise<WebUiConfigResponse> {
  const response = await fetch(apiUrl("/webui/config"), {
    method: "GET",
    headers: apiHeaders(),
  });
  return readJsonResponse<WebUiConfigResponse>(response);
}

export async function saveWebUiConfig({
  config,
  baseVersion,
}: SaveWebUiConfigOptions): Promise<WebUiConfigResponse> {
  const response = await fetch(apiUrl("/webui/config"), {
    method: "PUT",
    headers: {
      ...apiHeaders(),
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      config,
      base_version: baseVersion ?? undefined,
    }),
  });
  return readJsonResponse<WebUiConfigResponse>(response);
}

export async function patchWebUiConfig({
  patch,
  baseVersion,
}: PatchWebUiConfigOptions): Promise<WebUiConfigResponse> {
  const response = await fetch(apiUrl("/webui/config"), {
    method: "PATCH",
    headers: {
      ...apiHeaders(),
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      patch,
      base_version: baseVersion ?? undefined,
    }),
  });
  return readJsonResponse<WebUiConfigResponse>(response);
}

export async function deleteWebUiConfig(
  options: DeleteWebUiConfigOptions = {},
): Promise<WebUiConfigResponse> {
  const response = await fetch(apiUrl("/webui/config"), {
    method: "DELETE",
    headers: {
      ...apiHeaders(),
      "Content-Type": "application/json",
    },
    body:
      options.baseVersion === undefined
        ? undefined
        : JSON.stringify({ base_version: options.baseVersion }),
  });
  return readJsonResponse<WebUiConfigResponse>(response);
}

export async function fetchWebUiOptions(): Promise<WebUiOptionsResponse> {
  const response = await fetch(apiUrl("/webui/options"), {
    method: "GET",
    headers: apiHeaders(),
  });
  return readJsonResponse<WebUiOptionsResponse>(response);
}

export async function fetchHealth(): Promise<HealthResponse> {
  const response = await fetch(apiUrl("/health"), {
    method: "GET",
    headers: apiHeaders(),
  });
  return readJsonResponse<HealthResponse>(response);
}

export async function fetchBuildInfo(): Promise<BuildInfoResponse> {
  const response = await fetch(apiUrl("/build"), {
    method: "GET",
    headers: apiHeaders(),
  });
  return readJsonResponse<BuildInfoResponse>(response);
}

export async function fetchControl(): Promise<ControlResponse> {
  const response = await fetch(apiUrl("/control"), {
    method: "GET",
    headers: apiHeaders(),
  });
  return readJsonResponse<ControlResponse>(response);
}

export async function fetchSystem(): Promise<SystemResponse> {
  const response = await fetch(apiUrl("/system"), {
    method: "GET",
    headers: apiHeaders(),
  });
  return readJsonResponse<SystemResponse>(response);
}

export async function fetchReloadStatus(): Promise<ReloadSnapshot> {
  const response = await fetch(apiUrl("/reload/status"), {
    method: "GET",
    headers: apiHeaders(),
  });
  return readJsonResponse<ReloadSnapshot>(response);
}

export async function validateConfigText(
  content: string,
): Promise<ConfigValidateResponse> {
  const response = await fetch(apiUrl("/config/validate"), {
    method: "POST",
    headers: {
      ...apiHeaders(),
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ format: "yaml", content }),
  });
  return readJsonResponse<ConfigValidateResponse>(response);
}

export async function saveConfigFile({
  content,
  baseVersion,
  validate = true,
  reload = false,
}: SaveConfigOptions): Promise<SaveConfigResponse> {
  const response = await fetch(apiUrl("/config"), {
    method: "PUT",
    headers: {
      ...apiHeaders(),
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      format: "yaml",
      content,
      base_version: baseVersion ?? undefined,
      validate,
      reload,
    }),
  });
  return readJsonResponse<SaveConfigResponse>(response);
}

export async function requestReload(): Promise<void> {
  const response = await fetch(apiUrl("/reload"), {
    method: "POST",
    headers: apiHeaders(),
  });
  await readJsonResponse<unknown>(response);
}

export async function requestRestart(): Promise<void> {
  const response = await fetch(apiUrl("/restart"), {
    method: "POST",
    headers: apiHeaders(),
  });
  await readJsonResponse<unknown>(response);
}

export async function fetchCacheEntries(
  tag: string,
  options: { limit?: number; cursor?: string; qname?: string } = {},
): Promise<CacheEntriesResponse> {
  const params = new URLSearchParams();
  if (options.limit) params.set("limit", String(options.limit));
  if (options.cursor) params.set("cursor", options.cursor);
  if (options.qname) params.set("qname", options.qname);
  const suffix = params.toString() ? `?${params.toString()}` : "";
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/entries${suffix}`),
    { method: "GET", headers: apiHeaders() },
  );
  return readJsonResponse<CacheEntriesResponse>(response);
}

export async function deleteCacheEntry(tag: string, id: string): Promise<void> {
  const response = await fetch(
    apiUrl(
      `/plugins/${encodeURIComponent(tag)}/entries/${encodeURIComponent(id)}`,
    ),
    { method: "DELETE", headers: apiHeaders() },
  );
  await readJsonResponse<unknown>(response);
}

export async function flushCache(tag: string): Promise<void> {
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/flush`),
    {
      method: "GET",
      headers: apiHeaders(),
    },
  );
  await readJsonResponse<unknown>(response);
}

export async function fetchCacheDump(tag: string): Promise<Blob> {
  const { serverConfig } = useAuthStore.getState();
  const headers: Record<string, string> = {};
  if (serverConfig.requiresAuth && serverConfig.username) {
    headers.Authorization = `Basic ${btoa(`${serverConfig.username}:${serverConfig.password}`)}`;
  }
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/dump`),
    { method: "GET", headers },
  );
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.blob();
}

export interface CacheLoadDumpResponse {
  ok: boolean;
  loaded_entries: number;
}

export async function loadCacheDump(
  tag: string,
  data: ArrayBuffer,
): Promise<CacheLoadDumpResponse> {
  const { serverConfig } = useAuthStore.getState();
  const headers: Record<string, string> = {
    Accept: "application/json",
    "Content-Type": "application/octet-stream",
  };
  if (serverConfig.requiresAuth && serverConfig.username) {
    headers.Authorization = `Basic ${btoa(`${serverConfig.username}:${serverConfig.password}`)}`;
  }
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/load_dump`),
    { method: "POST", headers, body: data },
  );
  return readJsonResponse<CacheLoadDumpResponse>(response);
}

export async function fetchQueryRecords(
  tag: string,
  options: QueryRecordFilters & {
    limit?: number;
    cursor?: string;
    signal?: AbortSignal;
  } = {},
): Promise<QueryRecordsResponse> {
  const params = new URLSearchParams();
  if (options.limit) params.set("limit", String(options.limit));
  if (options.cursor) params.set("cursor", options.cursor);
  appendQueryRecordFilters(params, options);
  const suffix = params.toString() ? `?${params.toString()}` : "";
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/records${suffix}`),
    { method: "GET", headers: apiHeaders(), signal: options.signal },
  );
  return readJsonResponse<QueryRecordsResponse>(response);
}

export async function fetchQueryRecorderPluginStats(
  tag: string,
  options: QueryRecordFilters & {
    kind?: QueryRecorderPluginStatsKind;
    signal?: AbortSignal;
  } = {},
): Promise<QueryRecorderPluginStatsResponse> {
  const params = new URLSearchParams();
  params.set("kind", options.kind ?? "all");
  appendQueryRecordFilters(params, options);
  const suffix = params.toString() ? `?${params.toString()}` : "";
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/stats/plugins${suffix}`),
    { method: "GET", headers: apiHeaders(), signal: options.signal },
  );
  return readJsonResponse<QueryRecorderPluginStatsResponse>(response);
}

export async function fetchQueryRecordDetail(
  tag: string,
  id: number,
): Promise<QueryRecordDetailResponse> {
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/records/${id}`),
    { method: "GET", headers: apiHeaders() },
  );
  return readJsonResponse<QueryRecordDetailResponse>(response);
}

export async function clearQueryRecorderHistory(
  tag: string,
): Promise<QueryRecorderClearResponse> {
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/records`),
    { method: "DELETE", headers: apiHeaders() },
  );
  return readJsonResponse<QueryRecorderClearResponse>(response);
}

export async function fetchQueryRecorderTopClients(
  tag: string,
  options: QueryRecordFilters & { limit?: number; signal?: AbortSignal } = {},
): Promise<QueryRecorderTopResponse> {
  const params = new URLSearchParams();
  if (options.limit) params.set("limit", String(options.limit));
  appendQueryRecordFilters(params, options);
  const suffix = params.toString() ? `?${params.toString()}` : "";
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/stats/top_clients${suffix}`),
    { method: "GET", headers: apiHeaders(), signal: options.signal },
  );
  return readJsonResponse<QueryRecorderTopResponse>(response);
}

export async function fetchQueryRecorderTopQnames(
  tag: string,
  options: QueryRecordFilters & { limit?: number; signal?: AbortSignal } = {},
): Promise<QueryRecorderTopResponse> {
  const params = new URLSearchParams();
  if (options.limit) params.set("limit", String(options.limit));
  appendQueryRecordFilters(params, options);
  const suffix = params.toString() ? `?${params.toString()}` : "";
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/stats/top_qnames${suffix}`),
    { method: "GET", headers: apiHeaders(), signal: options.signal },
  );
  return readJsonResponse<QueryRecorderTopResponse>(response);
}

export async function fetchQueryRecorderQtypeDistribution(
  tag: string,
  options: QueryRecordFilters & { signal?: AbortSignal } = {},
): Promise<QueryRecorderDistributionResponse> {
  const params = new URLSearchParams();
  appendQueryRecordFilters(params, options);
  const suffix = params.toString() ? `?${params.toString()}` : "";
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/stats/qtype${suffix}`),
    { method: "GET", headers: apiHeaders(), signal: options.signal },
  );
  return readJsonResponse<QueryRecorderDistributionResponse>(response);
}

export async function fetchQueryRecorderRcodeDistribution(
  tag: string,
  options: QueryRecordFilters & { signal?: AbortSignal } = {},
): Promise<QueryRecorderDistributionResponse> {
  const params = new URLSearchParams();
  appendQueryRecordFilters(params, options);
  const suffix = params.toString() ? `?${params.toString()}` : "";
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/stats/rcode${suffix}`),
    { method: "GET", headers: apiHeaders(), signal: options.signal },
  );
  return readJsonResponse<QueryRecorderDistributionResponse>(response);
}

export async function fetchQueryRecorderLatency(
  tag: string,
  options: QueryRecordFilters & {
    slowLimit?: number;
    signal?: AbortSignal;
  } = {},
): Promise<QueryRecorderLatencySummary> {
  const params = new URLSearchParams();
  if (options.slowLimit) params.set("slow_limit", String(options.slowLimit));
  appendQueryRecordFilters(params, options);
  const suffix = params.toString() ? `?${params.toString()}` : "";
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/stats/latency${suffix}`),
    { method: "GET", headers: apiHeaders(), signal: options.signal },
  );
  return readJsonResponse<QueryRecorderLatencySummary>(response);
}

export async function fetchQueryRecorderTimeseries(
  tag: string,
  options: QueryRecordFilters & {
    bucket?: QueryRecorderTimeseriesBucket;
    buckets?: number;
    signal?: AbortSignal;
  } = {},
): Promise<QueryRecorderTimeseriesResponse> {
  const params = new URLSearchParams();
  if (options.bucket) params.set("bucket", options.bucket);
  if (options.buckets) params.set("buckets", String(options.buckets));
  appendQueryRecordFilters(params, options);
  const suffix = params.toString() ? `?${params.toString()}` : "";
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/stats/timeseries${suffix}`),
    { method: "GET", headers: apiHeaders(), signal: options.signal },
  );
  return readJsonResponse<QueryRecorderTimeseriesResponse>(response);
}

// --- Dynamic Domain Set API ---

export type DynamicDomainRuleKind = "full" | "domain";

export interface DynamicDomainRulesResponse {
  ok: boolean;
  total: number;
  next_cursor: number | null;
  rules: string[];
}

export interface DynamicDomainMutationResponse {
  ok: boolean;
  added: number;
  removed: number;
  total: number;
}

export async function listDynamicDomainRules(
  tag: string,
  options: { cursor?: number; limit?: number; signal?: AbortSignal } = {},
): Promise<DynamicDomainRulesResponse> {
  const params = new URLSearchParams();
  if (options.cursor !== undefined)
    params.set("cursor", String(options.cursor));
  if (options.limit !== undefined) params.set("limit", String(options.limit));
  const suffix = params.toString() ? `?${params.toString()}` : "";
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/rules${suffix}`),
    { method: "GET", headers: apiHeaders(), signal: options.signal },
  );
  return readJsonResponse<DynamicDomainRulesResponse>(response);
}

export async function appendDynamicDomainRules(
  tag: string,
  rules: string[],
  rule_kind?: DynamicDomainRuleKind,
): Promise<DynamicDomainMutationResponse> {
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/rules`),
    {
      method: "POST",
      headers: { ...apiHeaders(), "Content-Type": "application/json" },
      body: JSON.stringify({ rules, rule_kind }),
    },
  );
  return readJsonResponse<DynamicDomainMutationResponse>(response);
}

export async function removeDynamicDomainRules(
  tag: string,
  rules: string[],
  rule_kind?: DynamicDomainRuleKind,
): Promise<DynamicDomainMutationResponse> {
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/rules`),
    {
      method: "DELETE",
      headers: { ...apiHeaders(), "Content-Type": "application/json" },
      body: JSON.stringify({ rules, rule_kind }),
    },
  );
  return readJsonResponse<DynamicDomainMutationResponse>(response);
}

export async function clearDynamicDomainRules(
  tag: string,
): Promise<DynamicDomainMutationResponse> {
  const response = await fetch(
    apiUrl(`/plugins/${encodeURIComponent(tag)}/rules/clear`),
    { method: "POST", headers: apiHeaders() },
  );
  return readJsonResponse<DynamicDomainMutationResponse>(response);
}

// --- Log API ---

export interface LogEntry {
  id: number;
  timestamp: string;
  elapsed_ms: number;
  level: "ERROR" | "WARN" | "INFO" | "DEBUG" | "TRACE";
  target: string;
  message: string;
}

export interface LogsResponse {
  ok: boolean;
  total: number;
  entries: LogEntry[];
}

export async function fetchRecentLogs(params?: {
  level?: string;
  limit?: number;
}): Promise<LogsResponse> {
  const query = new URLSearchParams();
  if (params?.level) query.set("level", params.level);
  if (params?.limit) query.set("limit", String(params.limit));
  const suffix = query.size > 0 ? `?${query}` : "";
  const response = await fetch(apiUrl(`/logs${suffix}`), {
    method: "GET",
    headers: apiHeaders(),
  });
  return readJsonResponse<LogsResponse>(response);
}

export async function streamLogs(
  params: { level?: string; tail?: number },
  onEntry: (entry: LogEntry) => void,
  signal: AbortSignal,
): Promise<void> {
  const query = new URLSearchParams();
  if (params.level) query.set("level", params.level);
  if (params.tail !== undefined) query.set("tail", String(params.tail));
  const suffix = query.size > 0 ? `?${query}` : "";
  const response = await fetch(apiUrl(`/logs/stream${suffix}`), {
    method: "GET",
    headers: { ...apiHeaders(), Accept: "text/event-stream" },
    signal,
  });
  if (response.status === 401) {
    useAuthStore.getState().logout();
    throw new Error(tClient(WEBUI.storeErrors.loginExpired));
  }
  if (!response.ok || !response.body) {
    throw new Error(`HTTP ${response.status}`);
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    const blocks = buf.split("\n\n");
    buf = blocks.pop() ?? "";
    for (const block of blocks) {
      if (!block.trim()) continue;
      for (const line of block.split("\n")) {
        if (line.startsWith("data: ")) {
          try {
            onEntry(JSON.parse(line.slice(6)) as LogEntry);
          } catch {
            // ignore malformed frames
          }
        }
      }
    }
  }
}

export async function fetchPrometheusMetrics(): Promise<string> {
  const response = await fetch(apiUrl("/metrics"), {
    method: "GET",
    headers: { ...apiHeaders(), Accept: "text/plain" },
  });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.text();
}

// --- Upgrade API ---

export interface UpgradeCheckOptions {
  repository?: string;
  bundle?: string;
  socks5?: string;
  allowPrerelease?: boolean;
  target?: string;
  githubToken?: string;
}

export interface UpgradeCheckResponse {
  ok: boolean;
  current_version: string;
  latest_version: string;
  update_available: boolean;
  asset_name: string;
  release_url: string;
}

export interface UpgradeApplyResponse {
  ok: boolean;
  action: string;
  status: string;
  message: string;
}

export async function fetchUpgradeCheck(
  options: UpgradeCheckOptions = {},
): Promise<UpgradeCheckResponse> {
  const body: Record<string, unknown> = {};
  if (options.repository) body.repository = options.repository;
  if (options.bundle) body.bundle = options.bundle;
  if (options.socks5) body.socks5 = options.socks5;
  if (options.allowPrerelease) body.allow_prerelease = true;
  if (options.target) body.target = options.target;
  if (options.githubToken) body.github_token = options.githubToken;
  const response = await fetch(apiUrl("/upgrade/check"), {
    method: "POST",
    headers: { ...apiHeaders(), "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return readJsonResponse<UpgradeCheckResponse>(response);
}

export async function triggerUpgradeApply(
  options: UpgradeCheckOptions = {},
): Promise<UpgradeApplyResponse> {
  const body: Record<string, unknown> = {};
  if (options.repository) body.repository = options.repository;
  if (options.bundle) body.bundle = options.bundle;
  if (options.socks5) body.socks5 = options.socks5;
  if (options.allowPrerelease) body.allow_prerelease = true;
  if (options.target) body.target = options.target;
  if (options.githubToken) body.github_token = options.githubToken;
  const response = await fetch(apiUrl("/upgrade/apply"), {
    method: "POST",
    headers: { ...apiHeaders(), "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return readJsonResponse<UpgradeApplyResponse>(response);
}

export function apiUrl(path: string) {
  const baseUrl = useAuthStore.getState().serverConfig.url.trim();
  return `${baseUrl.replace(/\/$/, "")}${path}`;
}

function appendQueryRecordFilters(
  params: URLSearchParams,
  options: QueryRecordFilters,
) {
  if (options.sinceMs !== undefined) {
    params.set("since_ms", String(options.sinceMs));
  }
  if (options.untilMs !== undefined) {
    params.set("until_ms", String(options.untilMs));
  }
  if (options.qname) params.set("qname", options.qname);
  if (options.qtype) params.set("qtype", options.qtype);
  if (options.clientIp) params.set("client_ip", options.clientIp);
  if (options.rcode) params.set("rcode", options.rcode);
  if (options.status && options.status !== "all") {
    params.set("status", options.status);
  }
  if (options.matcherTag) params.set("matcher_tag", options.matcherTag);
}

export function apiHeaders() {
  const { serverConfig } = useAuthStore.getState();
  const headers: Record<string, string> = { Accept: "application/json" };
  if (serverConfig.requiresAuth && serverConfig.username) {
    headers.Authorization = `Basic ${btoa(`${serverConfig.username}:${serverConfig.password}`)}`;
  }
  return headers;
}

async function readJsonResponse<T>(response: Response): Promise<T> {
  if (response.status === 401) {
    useAuthStore.getState().logout();
    throw new Error(tClient(WEBUI.storeErrors.loginExpired));
  }
  const text = await response.text();
  const parsed = parseJsonResponseText(text);
  const body = parsed.ok ? parsed.value : undefined;
  if (!response.ok) {
    const bodyRecord = isRecord(body) ? body : undefined;
    const message =
      typeof bodyRecord?.message === "string"
        ? bodyRecord.message
        : httpErrorMessage(response, parsed.ok ? undefined : parsed.preview);
    if (
      bodyRecord &&
      Array.isArray(bodyRecord.diagnostics) &&
      bodyRecord.diagnostics.every((item: unknown) => typeof item === "string")
    ) {
      throw new ConfigValidationError(
        message,
        bodyRecord.diagnostics,
        Array.isArray(bodyRecord.diagnostic_details)
          ? (bodyRecord.diagnostic_details as ConfigDiagnostic[])
          : [],
      );
    }
    throw new Error(message);
  }

  if (!parsed.ok) {
    throw new Error(
      tClient(WEBUI.storeErrors.invalidJsonResponse, {
        preview: parsed.preview,
      }),
    );
  }

  return body as T;
}

type JsonResponseParseResult =
  | { ok: true; value: unknown }
  | { ok: false; preview: string };

function parseJsonResponseText(text: string): JsonResponseParseResult {
  const trimmed = text.trim();
  if (!trimmed) return { ok: true, value: {} };

  try {
    return { ok: true, value: JSON.parse(trimmed) as unknown };
  } catch {
    return { ok: false, preview: previewResponseText(trimmed) };
  }
}

function httpErrorMessage(response: Response, preview?: string) {
  const status = response.statusText
    ? `HTTP ${response.status} ${response.statusText}`
    : `HTTP ${response.status}`;
  return preview ? `${status}: ${preview}` : status;
}

function previewResponseText(text: string) {
  const singleLine = text.replace(/\s+/g, " ").trim();
  return singleLine.length > 160
    ? `${singleLine.slice(0, 157)}...`
    : singleLine;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}
