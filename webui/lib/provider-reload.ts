/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import type { DependencyGraphReport } from "./oxidns-api";
import type { PluginInstance } from "./types";

export type ProviderReloadOutcome = "idle" | "success" | "error";

export interface ProviderReloadState {
  pending: boolean;
  outcome: ProviderReloadOutcome;
  error?: string;
}

/** Mirrors the runtime init plan: non-provider plugins are live roots. */
export function providerHasLiveDependents(
  tag: string,
  graph: DependencyGraphReport | null,
): boolean | null {
  if (!graph) return null;

  const edgesBySource = new Map<string, string[]>();
  for (const edge of graph.edges) {
    const targets = edgesBySource.get(edge.source_tag) ?? [];
    targets.push(edge.target_tag);
    edgesBySource.set(edge.source_tag, targets);
  }

  const liveTags = new Set(
    graph.nodes
      .filter((node) => node.kind !== "provider")
      .map((node) => node.tag),
  );
  const pending = [...liveTags];
  while (pending.length > 0) {
    const source = pending.pop();
    if (!source) continue;
    for (const target of edgesBySource.get(source) ?? []) {
      if (liveTags.has(target)) continue;
      liveTags.add(target);
      pending.push(target);
    }
  }

  return liveTags.has(tag);
}

export function reconcileProviderReloads(
  plugins: PluginInstance[],
  current: Record<string, ProviderReloadState>,
): Record<string, ProviderReloadState> {
  return Object.fromEntries(
    plugins
      .filter((plugin) => plugin.type === "provider")
      .map((plugin) => [
        plugin.name,
        current[plugin.name] ?? {
          pending: false,
          outcome: "idle" as const,
        },
      ]),
  );
}
