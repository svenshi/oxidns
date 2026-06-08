"use client";

import { useMemo } from "react";
import { useAppStore } from "@/lib/store";
import { parseOxiDnsYaml } from "@/lib/oxidns-config";

export type PluginAppliedStatus = "applied" | "not-applied" | "unknown";

// Set of plugin tags present in the config the backend is currently running.
// `null` means we cannot determine the running config (offline mode, not
// connected, or the snapshot for `runningVersion` has been pruned). Components
// that care about "is this plugin live on the backend" should treat `null`
// as "unknown" rather than "not applied".
function useRunningPluginTags(): Set<string> | null {
  const runningVersion = useAppStore((s) => s.runningVersion);
  const configHistory = useAppStore((s) => s.configHistory);
  return useMemo(() => {
    if (!runningVersion) return null;
    const snapshot = configHistory.find(
      (entry) => entry.version === runningVersion,
    );
    if (!snapshot) return null;
    const parsed = parseOxiDnsYaml(snapshot.content);
    if (!parsed.config) return null;
    return new Set(
      parsed.config.plugins
        .map((plugin) => plugin.tag)
        .filter((tag): tag is string => Boolean(tag)),
    );
  }, [runningVersion, configHistory]);
}

// Tri-state per-plugin status: applied (tag present in running config),
// not-applied (running config known but tag missing — newly added or
// renamed), or unknown (no running config — offline mode / not connected).
export function usePluginAppliedStatus(
  tag: string | null | undefined,
): PluginAppliedStatus {
  const runningTags = useRunningPluginTags();
  if (!tag) return "unknown";
  if (runningTags === null) return "unknown";
  return runningTags.has(tag) ? "applied" : "not-applied";
}
