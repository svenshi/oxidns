"use client";

import { parseDocument, stringify, isSeq, isMap } from "yaml";
import { getPluginKindDefinition } from "@/lib/plugin-definitions";
import type { PluginInstance, PluginType } from "@/lib/types";

// Matches `${VAR}`, `${VAR:-default}`, `${env:VAR}` placeholders. The backend
// runs env expansion after YAML parsing, so we no longer need to coerce these
// strings into a particular scalar style — the YAML parser sees the literal
// placeholder text, expansion replaces it inside the typed value, and any
// special characters in the env value never touch the YAML grammar.
//
// The one thing we still have to clean up before serialization is the user's
// muscle-memory wrapping: form fields are plain text, so users who hit prior
// breakage often type `"${pw}"` or `'${pw}'` trying to "escape" the
// placeholder. Those quotes become part of the literal value, and the YAML
// stringifier adds its own wrapper around them — the runtime value then
// carries a stray pair of quotes. Stripping a single layer of matching ASCII
// quotes around a body that contains a placeholder removes that footgun.
const ENV_PLACEHOLDER_RE = /\$\{[^}]+\}/;

function stripStrayQuoteWrap(value: string): string {
  if (value.length < 2) return value;
  const first = value[0];
  if ((first !== '"' && first !== "'") || value[value.length - 1] !== first) {
    return value;
  }
  const inner = value.slice(1, -1);
  if (inner.includes(first)) return value;
  if (!ENV_PLACEHOLDER_RE.test(inner)) return value;
  return inner;
}

// Walk a JSON-ish value tree and strip the stray-quote wrapping from any
// string that carries an env-var placeholder.
function preserveEnvPlaceholders(value: unknown): unknown {
  if (typeof value === "string") return stripStrayQuoteWrap(value);
  if (Array.isArray(value)) return value.map(preserveEnvPlaceholders);
  if (value && typeof value === "object") {
    const result: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      result[k] = preserveEnvPlaceholders(v);
    }
    return result;
  }
  return value;
}

export interface OxiDnsConfig {
  include?: string[];
  runtime?: Record<string, unknown>;
  api?: Record<string, unknown>;
  log?: Record<string, unknown>;
  plugins: OxiDnsPluginConfig[];
  [key: string]: unknown;
}

export interface OxiDnsPluginConfig {
  tag: string;
  type: string;
  args?: unknown;
}

export interface OxiDnsParseResult {
  config?: OxiDnsConfig;
  diagnostics: string[];
}

const emptyMetrics = { calls: 0, avgLatency: 0, errorRate: 0, qps: 0 };

export function parseOxiDnsYaml(text: string): OxiDnsParseResult {
  try {
    const document = parseDocument(text, { prettyErrors: true });
    const diagnostics = [
      ...document.errors.map((error) => error.message),
      ...document.warnings.map((warning) => warning.message),
    ];
    if (document.errors.length > 0) return { diagnostics };

    const value = document.toJSON();
    if (!isPlainRecord(value)) {
      return { diagnostics: ["配置文件必须是 YAML 对象"] };
    }

    const rawPlugins = value.plugins;
    if (rawPlugins !== undefined && !Array.isArray(rawPlugins)) {
      return { diagnostics: ["plugins 必须是数组"] };
    }

    const plugins = (Array.isArray(rawPlugins) ? rawPlugins : []).map(
      (plugin, index): OxiDnsPluginConfig => {
        if (!isPlainRecord(plugin)) {
          throw new Error(`plugins[${index}] 必须是对象`);
        }
        return {
          tag: String(plugin.tag ?? ""),
          type: String(plugin.type ?? ""),
          args: plugin.args,
        };
      },
    );

    return {
      config: { ...value, plugins } as OxiDnsConfig,
      diagnostics,
    };
  } catch (error) {
    return {
      diagnostics: [error instanceof Error ? error.message : "YAML 解析失败"],
    };
  }
}

export function stringifyOxiDnsConfig(config: OxiDnsConfig): string {
  return stringify(preserveEnvPlaceholders(cleanUndefined(config)), {
    indent: 2,
    lineWidth: 0,
    nullStr: "null",
  });
}

// Re-serialize after a console-mode plugin edit while keeping the original
// file's comments and blank lines. Only the `plugins` value is rewritten;
// other top-level keys and the comment before `plugins:` are untouched.
//
// No value comparison: the caller passes the exact set of tags it changed.
// Every plugin whose tag is NOT in that set reuses its original YAML node
// verbatim (comments/blank lines fully preserved); only the edited plugin —
// and brand-new tags — are generated fresh. Falls back to a plain stringify
// if the previous text can't be parsed, so saving never breaks.
export function serializePluginsPreserving(
  prevText: string,
  config: OxiDnsConfig,
  changedTags: Set<string>,
): string {
  const newPlugins = config.plugins;
  // Fallback must serialize the WHOLE config — serializing only `plugins`
  // would wipe runtime/api/log/include when prevText is empty/unparseable.
  const fallback = (): string => stringifyOxiDnsConfig(config);
  if (!prevText || !prevText.trim()) return fallback();
  try {
    const doc = parseDocument(prevText);
    if (doc.errors.length > 0) return fallback();

    const oldNodeByTag = new Map<string, unknown>();
    const oldSeq = doc.get("plugins", true);
    if (isSeq(oldSeq)) {
      for (const item of oldSeq.items) {
        if (isMap(item)) {
          const tag = item.get("tag");
          if (typeof tag === "string" && !oldNodeByTag.has(tag)) {
            oldNodeByTag.set(tag, item);
          }
        }
      }
    }

    const items = newPlugins.map((p) => {
      const node = p.tag ? oldNodeByTag.get(p.tag) : undefined;
      if (node && isMap(node) && !changedTags.has(p.tag)) return node;
      return doc.createNode(preserveEnvPlaceholders(cleanUndefined(p)));
    });

    const seq = doc.createNode(
      newPlugins.map((p) => preserveEnvPlaceholders(cleanUndefined(p))),
    );
    if (isSeq(seq)) seq.items = items as typeof seq.items;
    doc.set("plugins", seq);
    return doc.toString({ lineWidth: 0 });
  } catch {
    return fallback();
  }
}

export function pluginsFromConfig(config: OxiDnsConfig): PluginInstance[] {
  return config.plugins.map((plugin) => {
    const definition = getPluginKindDefinition(plugin.type);
    const now = new Date().toISOString();
    return {
      id: plugin.tag || `${plugin.type}-${now}`,
      name: plugin.tag,
      type: definition?.type ?? inferPluginType(),
      pluginKind: plugin.type,
      status: "running",
      enabled: true,
      pinned: false,
      config: uiConfigFromPluginArgs(plugin.type, plugin.args),
      metrics: { ...emptyMetrics },
      createdAt: now,
      updatedAt: now,
    };
  });
}

export function configFromPlugins(
  baseConfig: OxiDnsConfig,
  plugins: PluginInstance[],
): OxiDnsConfig {
  return {
    ...baseConfig,
    plugins: plugins.map((plugin) => {
      const args = pluginArgsFromUiConfig(plugin.pluginKind, plugin.config);
      return {
        tag: plugin.name,
        type: plugin.pluginKind,
        ...(isEmptyValue(args) ? {} : { args }),
      };
    }),
  };
}

export function pluginConfigToYaml(config: unknown): string {
  return stringify(preserveEnvPlaceholders(cleanUndefined(config ?? {})), {
    indent: 2,
    lineWidth: 0,
    nullStr: "null",
  }).trimEnd();
}

export function pluginConfigFromYaml(input: string): {
  value?: Record<string, unknown>;
  error?: string;
} {
  const result = parseOxiDnsYaml(
    `plugins:\n  - tag: plugin\n    type: debug_print\n    args:\n${indentYaml(input || "{}", 6)}\n`,
  );
  if (result.diagnostics.length > 0 || !result.config) {
    return { error: result.diagnostics[0] ?? "YAML 解析失败" };
  }
  const args = result.config.plugins[0]?.args;
  if (!isPlainRecord(args)) return { error: "插件配置必须是 YAML 对象" };
  return { value: args };
}

export function uiConfigFromPluginArgs(
  pluginKind: string,
  args: unknown,
): Record<string, unknown> {
  const definition = getPluginKindDefinition(pluginKind);
  if (
    definition?.configSchema.length === 1 &&
    definition.configSchema[0].key === "args"
  ) {
    return { args: args ?? [] };
  }
  if (isPlainRecord(args)) return args;
  if (args === undefined || args === null) return {};
  return { args };
}

export function pluginArgsFromUiConfig(
  pluginKind: string,
  config: Record<string, unknown>,
): unknown {
  const definition = getPluginKindDefinition(pluginKind);
  if (
    definition?.configSchema.length === 1 &&
    definition.configSchema[0].key === "args"
  ) {
    return config.args;
  }
  return config;
}

// Compare two OxiDNS YAML configs and return true when anything outside the
// `plugins:` list differs. Top-level keys (runtime, api, log, include, …) only
// take effect on process start — they are NOT hot-reloadable. Used by the
// header sync control to switch the pending-change pill from "应用更改"
// (hot reload) to "需要重启" (full process restart) whenever the diff is
// load-bearing for restart-only fields.
export function topLevelConfigChanged(a: string, b: string): boolean {
  const left = stripPluginsForCompare(a);
  const right = stripPluginsForCompare(b);
  if (left === null || right === null) {
    // Unparseable input: fall back to a textual compare so the caller still
    // sees a difference and can prompt the safer (restart) action.
    return a.trim() !== b.trim();
  }
  return JSON.stringify(left) !== JSON.stringify(right);
}

function stripPluginsForCompare(text: string): Record<string, unknown> | null {
  const parsed = parseOxiDnsYaml(text);
  if (!parsed.config) return null;
  const rest: Record<string, unknown> = { ...parsed.config };
  delete rest.plugins;
  return rest;
}

export function createDefaultOxiDnsConfig(): OxiDnsConfig {
  return {
    log: { level: "info" },
    plugins: [],
  };
}

function inferPluginType(): PluginType {
  return "executor";
}

function cleanUndefined(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(cleanUndefined);
  if (!isPlainRecord(value)) return value;
  return Object.fromEntries(
    Object.entries(value)
      .filter(([, entry]) => entry !== undefined)
      .map(([key, entry]) => [key, cleanUndefined(entry)]),
  );
}

function isEmptyValue(value: unknown) {
  if (value === undefined || value === null) return true;
  if (Array.isArray(value)) return value.length === 0;
  return isPlainRecord(value) && Object.keys(value).length === 0;
}

function indentYaml(input: string, count: number) {
  const prefix = " ".repeat(count);
  return input
    .split("\n")
    .map((line) => `${prefix}${line}`)
    .join("\n");
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}
