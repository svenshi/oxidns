import { describe, expect, it } from "vitest";

import { createDefaultStandardSettings } from "./defaults";
import {
  selectStandardPathReferences,
  selectStandardUpstreamGroupReferences,
} from "./selectors";

describe("Standard Mode reference selectors", () => {
  it("lists resolution paths that reference an upstream group", () => {
    const settings = createDefaultStandardSettings();
    settings.paths.push({
      ...settings.paths[0],
      id: "private",
      name: "Private",
    });

    expect(
      selectStandardUpstreamGroupReferences(settings, "default").map(
        (reference) => reference.id,
      ),
    ).toEqual(["default", "private"]);
  });

  it("lists every path consumer, including disabled policies", () => {
    const settings = createDefaultStandardSettings();
    settings.routing.rules = [
      {
        id: "route_private",
        name: "Private route",
        enabled: false,
        condition: { type: "suffix", values: ["example.com"] },
        action: { type: "use_path", pathId: "default" },
        source: "manual",
      },
    ];
    settings.exceptions = [
      {
        id: "exception_private",
        name: "Private exception",
        enabled: true,
        condition: { type: "domain", values: ["host.example.com"] },
        action: { type: "use_path", pathId: "default" },
      },
    ];
    settings.devices = [
      {
        id: "phone",
        name: "Phone",
        addresses: ["192.0.2.10"],
        assignedPathId: "default",
      },
    ];
    settings.local.ddns = {
      enabled: false,
      domains: ["home.example.com"],
      pathId: "default",
      ttl: 30,
    };

    const references = selectStandardPathReferences(settings, "default");

    expect(references.map((reference) => reference.kind)).toEqual([
      "routing_rule",
      "exception",
      "device",
      "ddns",
    ]);
    expect(references[0]).toMatchObject({
      id: "route_private",
      enabled: false,
      href: "/standard/routing#rule-route_private",
    });
  });
});
