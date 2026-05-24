"use client";

import type { DependencyGraphEdge } from "@/lib/oxidns-api";
import type { OxiDnsConfig, OxiDnsPluginConfig } from "@/lib/oxidns-config";
import type { PluginInstance } from "@/lib/types";

export interface PluginReferenceImpact extends DependencyGraphEdge {
  sourcePlugin?: PluginInstance;
  removable: boolean;
  removeBlockedReason?: string;
}

export interface PluginReferenceMutationResult {
  config: OxiDnsConfig;
  changedTags: string[];
}

type PathPart = string | number;

const REMOVABLE_ARRAY_KEYS = new Set([
  "domain_set_tags",
  "executors",
  "ip_set_tags",
  "matchers",
  "provider_tags",
  "sets",
]);

export function getIncomingPluginReferences(
  plugins: PluginInstance[],
  edges: DependencyGraphEdge[] | undefined,
  targetTag: string,
): PluginReferenceImpact[] {
  if (!edges || !targetTag) return [];
  const pluginByTag = new Map(plugins.map((plugin) => [plugin.name, plugin]));

  return edges
    .filter((edge) => edge.target_tag === targetTag)
    .map((edge) => {
      const removable = isSafelyRemovableReference(edge);
      return {
        ...edge,
        sourcePlugin: pluginByTag.get(edge.source_tag),
        removable,
        removeBlockedReason: removable
          ? undefined
          : describeRemoveBlockReason(edge),
      };
    })
    .sort(compareReferenceImpact);
}

export function getReplacementCandidates(
  plugins: PluginInstance[],
  currentPluginId: string,
  references: PluginReferenceImpact[],
): PluginInstance[] {
  return plugins
    .filter((plugin) => plugin.id !== currentPluginId)
    .filter((plugin) =>
      references.every((reference) =>
        referenceAcceptsPlugin(reference, plugin),
      ),
    )
    .sort((a, b) => a.name.localeCompare(b.name));
}

export function replacePluginReferences(
  config: OxiDnsConfig,
  edges: DependencyGraphEdge[],
  oldTag: string,
  newTag: string,
): PluginReferenceMutationResult {
  const nextConfig = cloneConfig(config);
  const changedTags = new Set<string>();

  for (const edge of edges) {
    const source = findPluginConfig(nextConfig, edge.source_tag);
    if (!source) continue;
    const path = parseDependencyPath(edge.field);
    if (!path) continue;

    const current = getPathValue(source, path);
    const next = replaceReferenceValue(current, oldTag, newTag);
    if (next === current) continue;
    setPathValue(source, path, next);
    changedTags.add(edge.source_tag);
  }

  return { config: nextConfig, changedTags: [...changedTags] };
}

export function removeSafePluginReferences(
  config: OxiDnsConfig,
  edges: DependencyGraphEdge[],
): PluginReferenceMutationResult {
  const nextConfig = cloneConfig(config);
  const removals = new Map<string, { sourceTag: string; path: PathPart[] }>();
  const changedTags = new Set<string>();

  for (const edge of edges) {
    if (!isSafelyRemovableReference(edge)) continue;
    const path = parseDependencyPath(edge.field);
    if (!path || typeof path[path.length - 1] !== "number") continue;
    removals.set(`${edge.source_tag}:${path.join(".")}`, {
      sourceTag: edge.source_tag,
      path,
    });
  }

  const ordered = [...removals.values()].sort((left, right) => {
    if (left.sourceTag !== right.sourceTag) {
      return left.sourceTag.localeCompare(right.sourceTag);
    }
    const leftParent = left.path.slice(0, -1).join(".");
    const rightParent = right.path.slice(0, -1).join(".");
    if (leftParent !== rightParent)
      return leftParent.localeCompare(rightParent);
    return (
      Number(right.path[right.path.length - 1]) -
      Number(left.path[left.path.length - 1])
    );
  });

  for (const removal of ordered) {
    const source = findPluginConfig(nextConfig, removal.sourceTag);
    if (!source) continue;
    const parent = getPathValue(source, removal.path.slice(0, -1));
    const index = removal.path[removal.path.length - 1];
    if (!Array.isArray(parent) || typeof index !== "number") continue;
    if (index < 0 || index >= parent.length) continue;
    parent.splice(index, 1);
    changedTags.add(removal.sourceTag);
  }

  return { config: nextConfig, changedTags: [...changedTags] };
}

export function renamePluginConfigTag(
  config: OxiDnsConfig,
  oldTag: string,
  newTag: string,
): PluginReferenceMutationResult {
  const nextConfig = cloneConfig(config);
  const plugin = findPluginConfig(nextConfig, oldTag);
  if (!plugin) return { config: nextConfig, changedTags: [] };
  plugin.tag = newTag;
  return { config: nextConfig, changedTags: [oldTag, newTag] };
}

function cloneConfig(config: OxiDnsConfig): OxiDnsConfig {
  return JSON.parse(JSON.stringify(config)) as OxiDnsConfig;
}

function findPluginConfig(
  config: OxiDnsConfig,
  tag: string,
): OxiDnsPluginConfig | undefined {
  return config.plugins.find((plugin) => plugin.tag === tag);
}

function referenceAcceptsPlugin(
  reference: DependencyGraphEdge,
  plugin: PluginInstance,
) {
  const expectedKind = reference.expected_kind.toLowerCase();
  const kindMatches =
    expectedKind === "any" ||
    expectedKind === "unknown" ||
    expectedKind === plugin.type;
  const pluginTypeMatches =
    !reference.expected_plugin_type ||
    reference.expected_plugin_type === plugin.pluginKind;
  return kindMatches && pluginTypeMatches;
}

function parseDependencyPath(field: string): PathPart[] | null {
  const ownerField = field.split(" -> ")[0]?.trim();
  if (!ownerField || ownerField === "<unknown>") return null;

  const parts: PathPart[] = [];
  let index = 0;
  while (index < ownerField.length) {
    const char = ownerField[index];
    if (char === ".") {
      index += 1;
      continue;
    }
    if (char === "[") {
      const end = ownerField.indexOf("]", index);
      if (end < 0) return null;
      const raw = ownerField.slice(index + 1, end);
      if (!/^\d+$/.test(raw)) return null;
      parts.push(Number(raw));
      index = end + 1;
      continue;
    }
    const match = /^[A-Za-z_][A-Za-z0-9_]*/.exec(ownerField.slice(index));
    if (!match) return null;
    parts.push(match[0]);
    index += match[0].length;
  }

  return parts.length > 0 ? parts : null;
}

function getPathValue(root: unknown, path: PathPart[]): unknown {
  let current = root;
  for (const part of path) {
    if (current == null) return undefined;
    current = (current as Record<string, unknown> | unknown[])[part as never];
  }
  return current;
}

function setPathValue(root: unknown, path: PathPart[], value: unknown) {
  if (path.length === 0) return;
  const parent = getPathValue(root, path.slice(0, -1));
  if (parent == null) return;
  const key = path[path.length - 1];
  (parent as Record<string, unknown> | unknown[])[key as never] =
    value as never;
}

function replaceReferenceValue(value: unknown, oldTag: string, newTag: string) {
  if (typeof value !== "string") return value;
  if (value === oldTag) return newTag;
  if (value === `$${oldTag}`) return `$${newTag}`;
  if (value === `!$${oldTag}`) return `!$${newTag}`;

  const controlMatch = /^(jump|goto)\s+(\S+)$/.exec(value.trim());
  if (controlMatch?.[2] === oldTag) {
    return `${controlMatch[1]} ${newTag}`;
  }

  return replaceDollarReferenceTokens(value, oldTag, newTag);
}

function replaceDollarReferenceTokens(
  value: string,
  oldTag: string,
  newTag: string,
) {
  const pattern = new RegExp(
    `(^|[^A-Za-z0-9_-])\\$${escapeRegExp(oldTag)}(?=$|[^A-Za-z0-9_-])`,
    "g",
  );
  return value.replace(pattern, `$1$${newTag}`);
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function isSafelyRemovableReference(edge: DependencyGraphEdge) {
  if (edge.field.includes(" -> ")) return false;
  const path = parseDependencyPath(edge.field);
  if (!path || path.length < 2) return false;
  const index = path[path.length - 1];
  const parent = path[path.length - 2];
  return (
    typeof index === "number" &&
    typeof parent === "string" &&
    REMOVABLE_ARRAY_KEYS.has(parent)
  );
}

function describeRemoveBlockReason(edge: DependencyGraphEdge) {
  if (edge.field.includes(" -> ")) {
    return "快捷配置中的嵌套引用需要手动调整或替换";
  }
  if (edge.field.endsWith(".exec") || edge.field.includes(".entry")) {
    return "入口执行器和执行动作不能安全移除";
  }
  return "该字段不是可安全移除的引用数组项";
}

function compareReferenceImpact(
  left: PluginReferenceImpact,
  right: PluginReferenceImpact,
) {
  if (left.source_tag !== right.source_tag) {
    return left.source_tag.localeCompare(right.source_tag);
  }
  return left.field.localeCompare(right.field);
}
