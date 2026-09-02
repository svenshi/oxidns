import { isMap, isSeq, LineCounter, parseDocument } from "yaml";

import { pluginKindDefinitions } from "@/lib/plugin-definitions";
import type { PluginType } from "@/lib/types";

export interface PluginIndexEntry {
  tag: string;
  kind: string;
  category: PluginType | "unknown";
  line: number;
}

const kindToCategory = new Map<string, PluginType>(
  pluginKindDefinitions.map((definition) => [definition.kind, definition.type]),
);

function scalarText(value: unknown): string | undefined {
  if (
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  ) {
    return String(value);
  }
  return undefined;
}

export function parsePluginsFromYaml(text: string): PluginIndexEntry[] {
  try {
    const lineCounter = new LineCounter();
    const document = parseDocument(text, { lineCounter });
    const plugins = document.get("plugins", true);
    if (!isSeq(plugins)) return [];

    const results: PluginIndexEntry[] = [];
    for (const item of plugins.items) {
      if (!isMap(item)) continue;

      const tag = scalarText(item.get("tag"));
      if (!tag) continue;

      const kind = scalarText(item.get("type")) ?? "";
      const offset = item.range?.[0];
      results.push({
        tag,
        kind,
        category: kindToCategory.get(kind) ?? "unknown",
        line: typeof offset === "number" ? lineCounter.linePos(offset).line : 1,
      });
    }
    return results;
  } catch {
    return [];
  }
}
