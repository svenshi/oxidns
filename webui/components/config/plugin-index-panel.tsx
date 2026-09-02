"use client";

import { useMemo, useState } from "react";
import { Copy, Check, Server, Zap, Filter, Database } from "lucide-react";
import type { PluginType } from "@/lib/types";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import {
  parsePluginsFromYaml,
  type PluginIndexEntry,
} from "@/components/config/plugin-index";

const CATEGORY_ORDER: (PluginType | "unknown")[] = [
  "server",
  "executor",
  "matcher",
  "provider",
  "unknown",
];

const CATEGORY_ICONS: Record<PluginType | "unknown", React.ReactNode> = {
  server: <Server className="h-3 w-3" />,
  executor: <Zap className="h-3 w-3" />,
  matcher: <Filter className="h-3 w-3" />,
  provider: <Database className="h-3 w-3" />,
  unknown: null,
};

interface PluginIndexPanelProps {
  yamlText: string;
  onJumpToLine?: (line: number) => void;
}

export function PluginIndexPanel({
  yamlText,
  onJumpToLine,
}: PluginIndexPanelProps) {
  const { t } = useI18n();
  const [copiedTag, setCopiedTag] = useState<string | null>(null);

  const entries = useMemo(() => parsePluginsFromYaml(yamlText), [yamlText]);

  const grouped = useMemo(() => {
    const map = new Map<PluginType | "unknown", PluginIndexEntry[]>();
    for (const entry of entries) {
      const list = map.get(entry.category) ?? [];
      list.push(entry);
      map.set(entry.category, list);
    }
    return map;
  }, [entries]);

  const handleCopy = (tag: string, e: React.MouseEvent) => {
    e.stopPropagation();
    void navigator.clipboard.writeText(`$${tag}`).then(() => {
      setCopiedTag(tag);
      setTimeout(() => setCopiedTag(null), 1500);
    });
  };

  if (entries.length === 0) {
    return (
      <div className="flex items-center justify-center h-16 text-xs text-muted-foreground">
        {t(WEBUI.configEditor.noPlugins)}
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {CATEGORY_ORDER.map((cat) => {
        const items = grouped.get(cat);
        if (!items?.length) return null;
        return (
          <div key={cat}>
            <div className="flex items-center gap-1.5 mb-1 px-1">
              <span className="text-muted-foreground/70">
                {CATEGORY_ICONS[cat]}
              </span>
              <span className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                {pluginCategoryLabel(cat, t)}
              </span>
              <span className="ml-auto text-xs tabular-nums text-muted-foreground/50">
                {items.length}
              </span>
            </div>
            <div className="space-y-0.5">
              {items.map((entry) => (
                <div
                  key={entry.tag}
                  className="group flex items-center gap-1 rounded px-1.5 py-1 cursor-pointer hover:bg-muted/60 transition-colors"
                  onClick={() => onJumpToLine?.(entry.line)}
                >
                  <span className="flex-1 truncate text-xs font-mono text-foreground/90">
                    {entry.tag}
                  </span>
                  <button
                    className="hidden group-hover:flex items-center p-0.5 rounded text-muted-foreground hover:text-foreground transition-colors flex-shrink-0"
                    onClick={(e) => handleCopy(entry.tag, e)}
                    title={t(WEBUI.configEditor.copyReferenceTitle, {
                      tag: entry.tag,
                    })}
                  >
                    {copiedTag === entry.tag ? (
                      <Check className="h-3 w-3 text-primary" />
                    ) : (
                      <Copy className="h-3 w-3" />
                    )}
                  </button>
                  <span className="text-xs text-muted-foreground/40 font-mono flex-shrink-0">
                    {entry.kind}
                  </span>
                </div>
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function pluginCategoryLabel(
  category: PluginType | "unknown",
  t: ReturnType<typeof useI18n>["t"],
) {
  switch (category) {
    case "server":
      return t(WEBUI.pluginTypes.server);
    case "executor":
      return t(WEBUI.pluginTypes.executor);
    case "matcher":
      return t(WEBUI.pluginTypes.matcher);
    case "provider":
      return t(WEBUI.pluginTypes.provider);
    case "unknown":
      return t(WEBUI.configEditor.unknownCategory);
  }
}
