/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { describe, expect, it } from "vitest";

import { providerHasLiveDependents } from "./provider-reload";

describe("provider runtime liveness", () => {
  it("follows provider dependencies from non-provider roots", () => {
    const graph = {
      nodes: [
        { tag: "server", plugin_type: "udp_server", kind: "server" },
        { tag: "live", plugin_type: "domain_set", kind: "provider" },
        { tag: "nested", plugin_type: "domain_set", kind: "provider" },
        { tag: "unused", plugin_type: "domain_set", kind: "provider" },
      ],
      edges: [
        {
          source_tag: "server",
          field: "entry",
          target_tag: "live",
          expected_kind: "executor",
        },
        {
          source_tag: "live",
          field: "sets[0]",
          target_tag: "nested",
          expected_kind: "provider",
        },
      ],
      init_order: [],
      sequence_flows: [],
    };

    expect(providerHasLiveDependents("live", graph)).toBe(true);
    expect(providerHasLiveDependents("nested", graph)).toBe(true);
    expect(providerHasLiveDependents("unused", graph)).toBe(false);
  });

  it("leaves liveness unknown until the graph is available", () => {
    expect(providerHasLiveDependents("provider", null)).toBeNull();
  });
});
