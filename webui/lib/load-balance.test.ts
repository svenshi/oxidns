/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { describe, expect, it } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  createPluginConfigFormValues,
  PluginConfigFieldsEditor,
  serializePluginConfigValues,
} from "@/components/plugins/plugin-config-fields-editor";
import { TooltipProvider } from "@/components/ui/tooltip";
import { I18nProvider } from "./i18n/provider";
import { getVisiblePluginConfigFields } from "./plugin-config-visibility";

import {
  getLocalizedPluginKindDefinition,
  getPluginKindDefinition,
} from "./plugin-definitions";
import {
  configFromPlugins,
  parseOxiDnsYaml,
  pluginsFromConfig,
  stringifyOxiDnsConfig,
} from "./oxidns-config";
import { patchPluginsYaml } from "./oxidns-config-patch";
import { replacePluginReferences } from "./plugin-reference-operations";
import type { DependencyGraphEdge } from "./oxidns-api";

const yaml = `# retained source comment
plugins:
  - tag: balance_main
    type: load_balance
    args:
      strategy: weighted_random
      executors:
        - exec: seq_dns_a # retained branch comment
          weight: 3
        - exec: seq_dns_b
`;

describe("load_balance configuration", () => {
  it("shows weights only for weighted random in editable and read-only branch views", () => {
    const fields = getPluginKindDefinition("load_balance")!.configSchema;
    for (const strategy of [
      undefined,
      "round_robin",
      "random",
      "weighted_random",
    ]) {
      const values = createPluginConfigFormValues(fields, {
        ...(strategy === undefined ? {} : { strategy }),
        executors: [
          { exec: "seq_dns_a", weight: 3 },
          { exec: "seq_dns_b", weight: 1 },
        ],
      });
      for (const readOnly of [false, true]) {
        for (const collapsed of [false, true]) {
          const html = renderToStaticMarkup(
            createElement(
              I18nProvider,
              null,
              createElement(
                TooltipProvider,
                null,
                createElement(PluginConfigFieldsEditor, {
                  fields,
                  plugins: [],
                  values,
                  onChange: () => {},
                  readOnly,
                  defaultArrayObjectCollapsed: collapsed,
                }),
              ),
            ),
          );
          expect(html.includes("权重")).toBe(strategy === "weighted_random");
          expect(html).toContain("seq_dns_a");
        }
      }
    }
  });

  it("retains configured weights while switching strategies and editing a hidden-weight branch", () => {
    const fields = getPluginKindDefinition("load_balance")!.configSchema;
    const values = createPluginConfigFormValues(fields, {
      strategy: "weighted_random",
      executors: [{ exec: "seq_dns_a", weight: 3 }, { exec: "seq_dns_b" }],
    });
    for (const strategy of ["round_robin", "random", "weighted_random"]) {
      values.strategy = strategy;
      const branches = values.executors as Record<string, unknown>[];
      branches[0] = { ...branches[0], exec: "seq_new" };
      const visible = getVisiblePluginConfigFields(fields, values);
      const item = visible.find((field) => field.key === "executors")!.item;
      if (item?.type !== "object") throw new Error("expected branch object");
      expect(item.fields.some((field) => field.key === "weight")).toBe(
        strategy === "weighted_random",
      );
      expect(serializePluginConfigValues(fields, values)).toEqual({
        strategy,
        executors: [{ exec: "seq_new", weight: 3 }, { exec: "seq_dns_b" }],
      });
    }
  });

  it("round-trips object targets and omitted weights through form and YAML", () => {
    const parsed = parseOxiDnsYaml(yaml);
    expect(parsed.diagnostics).toEqual([]);
    const config = parsed.config!;
    const plugins = pluginsFromConfig(config);
    expect(plugins[0].type).toBe("executor");
    expect(plugins[0].pluginKind).toBe("load_balance");
    const next = configFromPlugins(config, plugins);
    expect(next).toEqual(config);
    expect(parseOxiDnsYaml(stringifyOxiDnsConfig(next)).config).toEqual(config);
    expect(patchPluginsYaml(yaml, config, next)).toEqual({
      status: "patched",
      content: yaml,
    });
  });

  it("uses typed plain-tag references and complete localized field help", () => {
    const definition = getPluginKindDefinition("load_balance")!;
    expect(definition.quickSetup).toBeUndefined();
    expect(definition.configSchema[0].default).toBe("round_robin");
    expect(
      definition.configSchema[0].options?.map((option) => option.value),
    ).toEqual(["round_robin", "random", "weighted_random"]);
    for (const locale of ["zh-CN", "en-US"] as const) {
      const localized = getLocalizedPluginKindDefinition(
        "load_balance",
        locale,
      )!;
      const targets = localized.configSchema.find(
        (field) => field.key === "executors",
      )!;
      expect(targets.item?.type).toBe("object");
      if (targets.item?.type !== "object")
        throw new Error("expected object targets");
      const reference = targets.item.fields.find(
        (field) => field.key === "exec",
      )!;
      expect(reference.type).toBe("reference");
      expect(reference.referenceTypes).toEqual(["executor"]);
      expect(reference.referencePrefix).toBe("");
      expect(reference.required).toBe(true);
      for (const field of [...localized.configSchema, ...targets.item.fields]) {
        expect(field.docs).toBeTruthy();
      }
      expect(
        targets.item.fields.find((field) => field.key === "weight")?.default,
      ).toBe(1);
      expect(
        localized.metrics?.metricHelp?.load_balance_selected_total,
      ).toBeTruthy();
    }
  });

  it("replaces nested executor references without changing weights or comments", () => {
    const config = parseOxiDnsYaml(yaml).config!;
    const edge: DependencyGraphEdge = {
      source_tag: "balance_main",
      field: "args.executors[0].exec",
      target_tag: "seq_dns_a",
      expected_kind: "executor",
    };
    const next = replacePluginReferences(
      config,
      [edge],
      "seq_dns_a",
      "seq_new",
    ).config;
    const result = patchPluginsYaml(yaml, config, next);
    expect(result.status).toBe("patched");
    if (result.status !== "patched") throw new Error(result.reason);
    expect(result.content).toBe(
      yaml.replace("exec: seq_dns_a", "exec: seq_new"),
    );
  });
});
