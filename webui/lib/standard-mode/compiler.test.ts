import { parse } from "yaml";
import { describe, expect, it } from "vitest";

import type { BuildInfo } from "../oxidns-api";
import golden from "./fixtures/rust-golden.json";
import { compileStandardIntent, standardIntentRevision } from "./compiler";

type GoldenCase = {
  input: unknown;
  normalizedIntent?: unknown;
  diagnosticCodes?: string[];
  canApply?: boolean;
  yamlTree?: unknown;
  tagMap?: unknown;
  managedFiles?: string[] | null;
  explanation?: { capabilities?: BuildInfo["supported_plugins"] & { features?: string[] } };
};

const cases = golden.cases as unknown as Record<string, GoldenCase>;
const capabilitySource = cases.default.explanation!.capabilities!;
const build: BuildInfo = {
  version: "golden",
  bundle: "standard",
  enabled_bundles: ["standard"],
  enabled_features: capabilitySource.features ?? [],
  supported_plugins: {
    servers: capabilitySource.servers,
    executors: capabilitySource.executors,
    matchers: capabilitySource.matchers,
    providers: capabilitySource.providers,
  },
};

describe("browser Standard compiler", () => {
  for (const name of [
    "default",
    "strategy_fastest",
    "strategy_balanced",
    "strategy_prefer_positive",
    "strategy_consensus",
    "strategy_ordered_fallback",
    "cache_ecs",
    "filtering_local",
    "smart_routing",
    "dedicated",
    "dynamic_learning",
    "advanced",
  ]) {
    it(`matches the frozen Rust YAML semantics for ${name}`, async () => {
      const fixture = cases[name];
      const plan = await compileStandardIntent({ intent: fixture.input, build });
      expect(plan.canApply).toBe(true);
      expect(plan.normalizedIntent).toEqual(fixture.normalizedIntent);
      expect(withoutDerivedHashes(parse(plan.generated!.yaml))).toEqual(
        withoutDerivedHashes(fixture.yamlTree),
      );
      expect(plan.generated!.tagMap).toEqual(fixture.tagMap);
      expect(plan.generated!.managedFiles).toEqual(fixture.managedFiles);
    });
  }

  it("migrates schema 1-6 deterministically and only once", async () => {
    for (let schema = 1; schema <= 6; schema++) {
      const fixture = cases[`schema_${schema}`];
      const first = await compileStandardIntent({ intent: fixture.input, build });
      const second = await compileStandardIntent({ intent: first.normalizedIntent, build });
      expect(first.normalizedIntent).toEqual(fixture.normalizedIntent);
      expect(second.normalizedIntent).toEqual(first.normalizedIntent);
      expect(second.generated?.yaml).toEqual(first.generated?.yaml);
    }
  });

  it("produces deterministic SHA-256 intent revisions", async () => {
    const input = cases.default.input;
    await expect(standardIntentRevision(input)).resolves.toBe(
      await standardIntentRevision(structuredClone(input)),
    );
  });

  it("reports unavailable runtime plugins before apply", async () => {
    const plan = await compileStandardIntent({
      intent: cases.missing_capabilities.input,
      build: { ...build, enabled_features: [], supported_plugins: { servers: [], executors: [], matchers: [], providers: [] } },
    });
    expect(plan.canApply).toBe(false);
    expect(plan.diagnostics.some((item) => item.code === "required_capability_missing")).toBe(true);
  });
});

function withoutDerivedHashes<T>(value: T): T {
  const copy = structuredClone(value) as unknown;
  visit(copy);
  return copy as T;
}

function visit(value: unknown): void {
  if (Array.isArray(value)) {
    value.forEach(visit);
    return;
  }
  if (!value || typeof value !== "object") return;
  const record = value as Record<string, unknown>;
  if (typeof record.intentRevision === "string") {
    record.intentRevision = "<intent-revision>";
  }
  Object.values(record).forEach(visit);
}
