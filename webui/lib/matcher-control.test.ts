/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { describe, expect, it } from "vitest";

import {
  planMatcherModeChange,
  reconcileMatcherControls,
  type MatcherControlState,
} from "./matcher-control";
import type { PluginInstance, PluginType } from "./types";

function plugin(
  name: string,
  type: PluginType,
  enabled = true,
): PluginInstance {
  return {
    id: name,
    name,
    type,
    pluginKind: type === "matcher" ? "qname" : "forward",
    status: "running",
    enabled,
    pinned: false,
    config: {},
    metrics: { calls: 0, avgLatency: 0, errorRate: 0, qps: 0 },
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  };
}

describe("matcher runtime state", () => {
  it.each(["always_false", "always_true"] as const)(
    "requires confirmation before applying %s",
    (mode) => {
      expect(planMatcherModeChange(mode)).toEqual({ kind: "confirm", mode });
    },
  );

  it("restores normal mode without confirmation", () => {
    expect(planMatcherModeChange("normal")).toEqual({
      kind: "apply",
      mode: "normal",
    });
  });

  it("keeps known controls, adds unavailable matchers, and removes stale tags", () => {
    const ready: MatcherControlState = {
      availability: "ready",
      pending: false,
      mode: "always_false",
    };
    const controls = reconcileMatcherControls(
      [
        plugin("match_cn", "matcher"),
        plugin("new_matcher", "matcher"),
        plugin("exec", "executor"),
      ],
      { match_cn: ready, removed: ready },
    );

    expect(controls).toEqual({
      match_cn: ready,
      new_matcher: {
        availability: "unavailable",
        pending: false,
        mode: null,
      },
    });
  });
});
