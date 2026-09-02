"use client";

import { parseDocument, stringify } from "yaml";
import { sortOxiDnsConfigForSerialize } from "@/lib/oxidns-config-schema";
import { getPluginKindDefinition } from "@/lib/plugin-definitions";
import type { PluginInstance, PluginType } from "@/lib/types";
import { WEBUI, tClient } from "@/lib/i18n";

export interface OxiDnsConfig {
  include?: string[];
  runtime?: Record<string, unknown>;
  api?: Record<string, unknown>;
  log?: Record<string, unknown>;
  network?: Record<string, unknown>;
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
      return { diagnostics: [tClient(WEBUI.storeErrors.yamlRootMustBeObject)] };
    }

    const rawPlugins = value.plugins;
    if (rawPlugins !== undefined && !Array.isArray(rawPlugins)) {
      return { diagnostics: [tClient(WEBUI.storeErrors.pluginsMustBeArray)] };
    }

    const plugins = (Array.isArray(rawPlugins) ? rawPlugins : []).map(
      (plugin, index): OxiDnsPluginConfig => {
        if (!isPlainRecord(plugin)) {
          throw new Error(
            tClient(WEBUI.storeErrors.pluginEntryMustBeObject, { index }),
          );
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
      diagnostics: [
        error instanceof Error
          ? error.message
          : tClient(WEBUI.storeErrors.yamlParseFailed),
      ],
    };
  }
}

export function stringifyOxiDnsConfig(config: OxiDnsConfig): string {
  return stringify(sortOxiDnsConfigForSerialize(cleanUndefined(config)), {
    indent: 2,
    lineWidth: 0,
    nullStr: "null",
  });
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
  return stringify(cleanUndefined(config ?? {}), {
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
    return {
      error:
        result.diagnostics[0] ?? tClient(WEBUI.storeErrors.yamlParseFailed),
    };
  }
  const args = result.config.plugins[0]?.args;
  if (!isPlainRecord(args)) {
    return { error: tClient(WEBUI.plugins.yamlMustBeObject) };
  }
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
// header sync control to switch the pending-change pill from "apply changes"
// (hot reload) to "needs restart" (full process restart) whenever the diff is
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
