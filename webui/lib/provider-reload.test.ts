/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { describe, expect, it } from "vitest";

import {
  reconcileProviderReloads,
  type ProviderReloadState,
} from "./provider-reload";
import type { PluginInstance, PluginType } from "./types";

function plugin(name: string, type: PluginType): PluginInstance {
  return {
    id: name,
    name,
    type,
    pluginKind: type === "provider" ? "domain_set" : "forward",
    status: "running",
    enabled: true,
    pinned: false,
    config: {},
    metrics: { calls: 0, avgLatency: 0, errorRate: 0, qps: 0 },
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  };
}

describe("provider reload state", () => {
  it("keeps known outcomes, adds idle providers, and removes stale tags", () => {
    const success: ProviderReloadState = {
      pending: false,
      outcome: "success",
    };
    const states = reconcileProviderReloads(
      [
        plugin("domains", "provider"),
        plugin("new_provider", "provider"),
        plugin("exec", "executor"),
      ],
      { domains: success, removed: success },
    );

    expect(states).toEqual({
      domains: success,
      new_provider: {
        pending: false,
        outcome: "idle",
      },
    });
  });
});
