import { describe, expect, it } from "vitest";

import { parsePluginsFromYaml } from "@/components/config/plugin-index";

describe("configuration editor plugin index", () => {
  it("interprets quoted plugin tags and types as YAML scalars", () => {
    const entries = parsePluginsFromYaml(`
plugins:
  - tag: "ip_select"
    type: "ip_selector"
  - tag: 'main_sequence'
    type: 'sequence'
`);

    expect(entries).toEqual([
      {
        tag: "ip_select",
        kind: "ip_selector",
        category: "executor",
        line: 3,
      },
      {
        tag: "main_sequence",
        kind: "sequence",
        category: "executor",
        line: 5,
      },
    ]);
  });

  it("supports comments and flow-style plugin entries", () => {
    const entries = parsePluginsFromYaml(`plugins:
  - tag: forward # primary upstream
    type: forward # executor
  - { tag: "udp_server", type: "udp_server" }
`);

    expect(
      entries.map(({ tag, kind, category }) => ({
        tag,
        kind,
        category,
      })),
    ).toEqual([
      { tag: "forward", kind: "forward", category: "executor" },
      { tag: "udp_server", kind: "udp_server", category: "server" },
    ]);
  });

  it("keeps unrecognized plugin types visible in the unknown category", () => {
    expect(
      parsePluginsFromYaml(`plugins:
  - tag: custom
    type: third_party_plugin
`),
    ).toEqual([
      {
        tag: "custom",
        kind: "third_party_plugin",
        category: "unknown",
        line: 2,
      },
    ]);
  });
});
