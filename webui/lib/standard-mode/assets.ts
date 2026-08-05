import { parse } from "yaml";

import { normalizeServerUrl, useAuthStore } from "../auth-store";
import type {
  BuildInfo,
  DependencyGraphReport,
} from "../oxidns-api";
import { validateConfigText } from "../oxidns-api";
import { compileStandardIntent, standardIntentRevision } from "./compiler";
import { normalizeStandardSettings } from "./schema";
import type {
  StandardModeSettings,
  StandardDedicatedPathPolicy,
  StandardPlanResponse,
  StandardTemplateKind,
  StandardTemplateParameters,
  StandardTemplatePreviewResponse,
} from "./types";

export interface StandardAssetEnvelope {
  assetSchema: number;
  kind: "oxidns_standard_intent";
  oxidnsVersion: string;
  bundle: string;
  intentSchema: number;
  intentRevision: string;
  intent: StandardModeSettings;
  exportedAtMs: number;
  name?: string;
  description?: string;
}

export interface StandardSavedTemplate {
  id: string;
  name: string;
  description?: string;
  kind: StandardTemplateKind;
  parameters: StandardTemplateParameters;
  sourceIntentSchema: number;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface StandardAssetStore {
  schema: number;
  version: string;
  templates: StandardSavedTemplate[];
}

const DB_NAME = "oxidns-standard-assets";
const STORE_NAME = "templates";
const MAX_ENTRIES = 64;
const MAX_BYTES = 2 * 1024 * 1024;

export async function previewStandardTemplate(options: {
  baseIntent: StandardModeSettings;
  kind: StandardTemplateKind;
  parameters: StandardTemplateParameters;
  build: BuildInfo;
  baseYaml: string;
}): Promise<StandardTemplatePreviewResponse> {
  const expansion = expandTemplate(options.baseIntent, options.kind, options.parameters);
  const policy = await compileStandardIntent({
    intent: expansion.proposedIntent,
    baseYaml: options.baseYaml,
    build: options.build,
  });
  const generated = policy.generated;
  const validation = generated ? await validateConfigText(generated.yaml) : null;
  const plan: StandardPlanResponse = {
    ok: true,
    config_version: "client",
    standard_version: "client",
    ownership: "managed",
    semantic_diff: {
      preserved_top_level: ["include", "api", "network", "runtime", "log"],
      generated_plugin_tags: generated?.generatedTags ?? [],
      replaced_plugin_tags: [],
      removed_plugin_tags: [],
    },
    dependency_graph: validation?.dependency_graph,
    blockers: [],
    can_apply: policy.canApply && Boolean(validation),
    plan: policy,
  };
  return { ok: true, expansion, plan };
}

export function expandTemplate(
  base: StandardModeSettings,
  kind: StandardTemplateKind,
  parameters: StandardTemplateParameters,
): StandardTemplatePreviewResponse["expansion"] {
  const namespace = parameters.namespace.trim().toLowerCase();
  if (!/^[a-z0-9_-]+$/.test(namespace)) {
    throw new Error("template namespace must contain only letters, digits, '-' or '_'");
  }
  if (!parameters.name.trim() || !parameters.domains.length || !parameters.upstreams.length) {
    throw new Error("template requires a name, at least one domain, and at least one upstream");
  }
  const ids = [
    ...base.upstreamGroups,
    ...base.paths,
    ...base.dedicatedGroups,
    ...base.dynamicLearning.profiles,
    ...base.advancedRules,
    ...base.routing.rules,
    ...base.exceptions,
    ...base.devices,
  ].map((item) => item.id.toLowerCase());
  if (ids.includes(namespace)) throw new Error(`template namespace '${namespace}' collides with an existing object`);
  if (kind === "privacy_dns" && parameters.upstreams.some((item) => !["dot", "doh", "doh3", "doq"].includes(item.protocol))) {
    throw new Error("privacy_dns requires every upstream to use DoT, DoH, DoH3, or DoQ");
  }
  const basePath = base.paths[0];
  const path: StandardDedicatedPathPolicy = {
    filtering: "inherit" as const,
    cache: "inherit" as const,
    queryLog: "inherit" as const,
    dualStack: "inherit" as const,
    ipSelection: structuredClone(basePath.ipSelection),
    ecs: { mode: "inherit" } as const,
  };
  let strategy: StandardModeSettings["upstreamGroups"][number]["strategy"] = "balanced";
  let explanation = "regional_ecs_isolated";
  if (kind === "low_latency") {
    strategy = "fastest";
    path.ipSelection.enabled = true;
    path.ipSelection.selectionMode = "best_within_budget";
    explanation = "latency_optimized";
  } else if (kind === "privacy_dns") {
    strategy = "ordered_fallback";
    path.cache = "enabled";
    path.ecs = { mode: "remove" };
    explanation = "encrypted_ecs_removed";
  } else if (kind === "internal_domains") {
    strategy = "ordered_fallback";
    path.dualStack = "disabled";
    explanation = "internal_authority";
  } else {
    path.cache = "enabled";
    path.ecs = { mode: "client_subnet", mask4: 24, mask6: 56 };
  }
  const proposedIntent = structuredClone(base);
  proposedIntent.dedicatedGroups.push({
    id: namespace,
    name: parameters.name.trim(),
    description: parameters.description,
    enabled: true,
    priority: 100,
    rules: parameters.domains,
    strategy,
    upstreams: parameters.upstreams,
    path,
    listener: parameters.listenerAddress?.trim()
      ? { enabled: true, address: parameters.listenerAddress, udp: true, tcp: true }
      : { enabled: false, address: "127.0.0.1:5539", udp: true, tcp: true },
  });
  return {
    proposedIntent,
    objectsAdded: [`dedicatedGroups.${namespace}`],
    objectsModified: [],
    explanationTags: [explanation, `template:${namespace}`],
  };
}

export async function exportStandardAsset(
  intent: StandardModeSettings,
  build: BuildInfo,
): Promise<StandardAssetEnvelope> {
  const normalized = normalizeStandardSettings(intent).settings;
  return {
    assetSchema: 1,
    kind: "oxidns_standard_intent",
    oxidnsVersion: build.version,
    bundle: build.bundle,
    intentSchema: normalized.schema,
    intentRevision: await standardIntentRevision(normalized),
    intent: normalized,
    exportedAtMs: Date.now(),
  };
}

export function importStandardAsset(asset: StandardAssetEnvelope): StandardModeSettings {
  if (asset.assetSchema !== 1 || asset.kind !== "oxidns_standard_intent") {
    throw new Error("unsupported Standard intent asset");
  }
  const loaded = normalizeStandardSettings(asset.intent);
  if (loaded.notice === "invalid_fallback") throw new Error("Standard intent asset is invalid");
  return loaded.settings;
}

export async function copyStandardToExpert(
  intent: StandardModeSettings,
  build: BuildInfo,
  baseYaml: string,
) {
  const plan = await compileStandardIntent({ intent, build, baseYaml });
  if (!plan.generated) throw new Error(plan.diagnostics.map((item) => item.message).join("\n"));
  return {
    detached: true,
    yaml: plan.generated.yaml,
    configVersion: plan.generated.configVersion,
    intentRevision: plan.generated.explanation?.intentRevision ?? "",
  };
}

export async function analyzeExpertConfig(yaml: string): Promise<{
  pluginCount: number;
  dependencyGraph: DependencyGraphReport;
  expertOnlyObjects: Array<{ tag: string; pluginType: string; kind: string }>;
  systemIntegrations: string[];
  reverseConversion: { available: false; reason: string };
}> {
  const validation = await validateConfigText(yaml);
  const parsed = parse(yaml) as { plugins?: Array<{ tag?: string; type?: string }> };
  const integrations = new Set(["ipset", "nftset", "mikrotik"]);
  const plugins = parsed.plugins ?? [];
  return {
    pluginCount: validation.plugin_count,
    dependencyGraph: validation.dependency_graph,
    expertOnlyObjects: plugins
      .filter((plugin) => plugin.tag && plugin.type && !plugin.tag.startsWith("standard_"))
      .map((plugin) => ({ tag: plugin.tag!, pluginType: plugin.type!, kind: catalogKind(plugin.type!) })),
    systemIntegrations: plugins.filter((plugin) => plugin.type && integrations.has(plugin.type)).map((plugin) => plugin.tag ?? plugin.type!),
    reverseConversion: { available: false, reason: "Expert YAML remains native YAML and is not reverse-compiled into product intent" },
  };
}

export async function fetchSavedStandardTemplates(configPath = ""): Promise<{ ok: true; store: StandardAssetStore }> {
  const templates = await readTemplates(scopeKey(configPath));
  return { ok: true, store: await makeStore(templates) };
}

export async function saveStandardTemplate(template: StandardSavedTemplate, expectedVersion?: string, configPath = "") {
  return mutateTemplates(configPath, expectedVersion, (templates) => {
    if (templates.some((item) => item.id === template.id)) throw new Error("saved-template id already exists");
    const now = Date.now();
    templates.push({ ...template, createdAtMs: now, updatedAtMs: now });
  });
}

export async function duplicateStandardTemplate(id: string, newId: string, newName: string, expectedVersion?: string, configPath = "") {
  return mutateTemplates(configPath, expectedVersion, (templates) => {
    const source = templates.find((item) => item.id === id);
    if (!source) throw new Error("saved template does not exist");
    if (templates.some((item) => item.id === newId)) throw new Error("saved-template id already exists");
    const now = Date.now();
    templates.push({ ...structuredClone(source), id: newId, name: newName, parameters: { ...source.parameters, namespace: newId, name: newName }, createdAtMs: now, updatedAtMs: now });
  });
}

export async function deleteStandardTemplate(id: string, expectedVersion?: string, configPath = "") {
  return mutateTemplates(configPath, expectedVersion, (templates) => {
    const index = templates.findIndex((item) => item.id === id);
    if (index < 0) throw new Error("saved template does not exist");
    templates.splice(index, 1);
  });
}

async function mutateTemplates(configPath: string, expectedVersion: string | undefined, mutation: (templates: StandardSavedTemplate[]) => void) {
  const scope = scopeKey(configPath);
  const templates = await readTemplates(scope);
  const current = await makeStore(templates);
  if (expectedVersion && expectedVersion !== current.version) throw new Error("saved templates changed after they were loaded");
  mutation(templates);
  if (templates.length > MAX_ENTRIES) throw new Error("saved-template limit of 64 reached");
  const bytes = new TextEncoder().encode(JSON.stringify(templates)).length;
  if (bytes > MAX_BYTES) throw new Error("saved templates exceed 2 MiB");
  await writeTemplates(scope, templates);
  return { ok: true as const, store: await makeStore(templates) };
}

function scopeKey(configPath: string) {
  const url = normalizeServerUrl(useAuthStore.getState().serverConfig.url);
  return `${url}\0${configPath}`;
}

async function makeStore(templates: StandardSavedTemplate[]): Promise<StandardAssetStore> {
  const data = new TextEncoder().encode(JSON.stringify(templates));
  const digest = await crypto.subtle.digest("SHA-256", data);
  const version = [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
  return { schema: 1, version: `sha256:${version}`, templates: [...templates].sort((a, b) => a.id.localeCompare(b.id)) };
}

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE_NAME);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function readTemplates(scope: string): Promise<StandardSavedTemplate[]> {
  if (typeof indexedDB === "undefined") return [];
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const request = db.transaction(STORE_NAME, "readonly").objectStore(STORE_NAME).get(scope);
    request.onsuccess = () => resolve(Array.isArray(request.result) ? request.result : []);
    request.onerror = () => reject(request.error);
  });
}

async function writeTemplates(scope: string, templates: StandardSavedTemplate[]): Promise<void> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    tx.objectStore(STORE_NAME).put(templates, scope);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

function catalogKind(type: string): string {
  if (type.endsWith("_server")) return "server";
  if (["qname", "qtype", "client_ip", "resp_ip", "rcode", "cname", "time", "rate_limiter"].includes(type)) return "matcher";
  if (["domain_set", "dynamic_domain_set", "ip_set", "geosite", "geoip", "adguard_rule"].includes(type)) return "provider";
  return "executor";
}
