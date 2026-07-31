import { describe, expect, it } from "vitest";

import { createDefaultStandardSettings } from "./defaults";
import { normalizeStandardSettings } from "./schema";

describe("Standard Mode schema v3", () => {
  it("migrates v2 cache fields and removes inert Phase 2 values", () => {
    const legacy = createDefaultStandardSettings() as unknown as Record<
      string,
      unknown
    >;
    legacy.schema = 2;
    legacy.cache = {
      enabled: true,
      size: 2048,
      minTtl: 12,
      maxTtl: 1200,
      negativeTtl: 45,
    };
    legacy.queryLog = { enabled: true, retentionDays: 3, sampleRate: 0.25 };
    legacy.upstreamGroups = [
      {
        id: "default",
        name: "Default",
        strategy: "sequential",
        isDefault: true,
        upstreams: [
          {
            id: "local",
            name: "Local",
            protocol: "udp",
            address: "127.0.0.1:5353",
            enabled: true,
          },
        ],
      },
    ];
    legacy.paths = [
      {
        id: "default",
        name: "Default",
        upstreamGroupId: "missing",
        filtering: "inherit",
        cache: "inherit",
        queryLog: "inherit",
        dualStack: "prefer_ipv4",
        ipSelection: "enabled",
        ecs: "enabled",
      },
    ];
    legacy.routing = {
      enabled: false,
      rules: [],
      scenarios: [
        { id: "privacy", name: "Privacy", enabled: true, kind: "privacy" },
      ],
    };

    const result = normalizeStandardSettings(legacy);

    expect(result.notice).toBe("legacy_migrated");
    expect(result.settings).toMatchObject({
      schema: 3,
      cache: {
        minPositiveTtl: 12,
        maxPositiveTtl: 1200,
        maxNegativeTtl: 45,
        negativeTtlWithoutSoa: 45,
      },
      queryLog: { sampleRate: 1 },
      upstreamGroups: [{ strategy: "balanced" }],
      paths: [
        {
          upstreamGroupId: "missing",
          dualStack: "inherit",
          ipSelection: "inherit",
          ecs: "inherit",
        },
      ],
      routing: { scenarios: [] },
    });
  });

  it("preserves v3 invalid references and duplicate ids for backend diagnostics", () => {
    const current = createDefaultStandardSettings();
    current.paths[0].upstreamGroupId = "missing";
    current.paths.push({ ...current.paths[0] });

    const result = normalizeStandardSettings(current);

    expect(result.notice).toBeNull();
    expect(result.settings.paths).toHaveLength(2);
    expect(result.settings.paths[0].upstreamGroupId).toBe("missing");
  });
});
