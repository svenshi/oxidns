/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { CompletionContext } from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";

import { getPluginKindDefinition } from "@/lib/plugin-definitions";
import {
  getOxiDnsYamlCompletions,
  type OxiDnsYamlEditorContext,
} from "@/lib/oxidns-yaml-codemirror";

// A nested fixture protects generic completion depth independently of plugin schemas.
const context: OxiDnsYamlEditorContext = {
  variant: "plugin-args",
  fields: [
    {
      key: "edns",
      label: "EDNS",
      type: "object",
      fields: [
        {
          key: "options",
          label: "Options",
          type: "object",
          fields: [
            {
              key: "policy",
              label: "Policy",
              type: "select",
              options: [
                { label: "Preserve", value: "preserve" },
                { label: "Strip", value: "strip" },
                { label: "Known", value: "strip_unknown" },
              ],
            },
            {
              key: "allowlist",
              label: "Allowlist",
              type: "array",
              item: { type: "number" },
            },
            {
              key: "denylist",
              label: "Denylist",
              type: "array",
              item: { type: "number" },
            },
          ],
        },
      ],
    },
  ],
};

function labels(doc: string, editorContext = context, pos = doc.length) {
  const state = EditorState.create({ doc });
  const result = getOxiDnsYamlCompletions(
    new CompletionContext(state, pos, true),
    editorContext,
  );
  return result?.options.map((option) => option.label) ?? [];
}

describe("nested schema YAML completions", () => {
  it.each(["", "pol"])("completes an unfinished option key %j", (key) => {
    expect(labels(`args:\n  edns:\n    options:\n      ${key}`)).toEqual([
      "policy",
      "allowlist",
      "denylist",
    ]);
  });

  it("completes a parent object's children", () => {
    expect(labels("args:\n  edns:\n    ")).toEqual(["options"]);
  });

  it("completes upstream object fields inside the full configuration", () => {
    const result = labels(
      "plugins:\n  - tag: forward_main\n    type: forward\n    args:\n      upstreams:\n        - ",
      { variant: "config" },
    );
    expect(result).toContain("addr");
    expect(result).toContain("bootstrap");
    expect(result).not.toContain("upstreams");
  });

  it("keeps policy values and existing key edits at the correct schema depth", () => {
    expect(labels("args:\n  edns:\n    options:\n      policy: ")).toEqual([
      "preserve",
      "strip",
      "strip_unknown",
    ]);
    const doc = "args:\n  edns:\n    options:\n      policy: preserve";
    expect(labels(doc, context, doc.indexOf("policy") + 3)).toEqual([
      "policy",
      "allowlist",
      "denylist",
    ]);
  });

  it("continues completing fields in upstream array objects", () => {
    const result = labels("args:\n  upstreams:\n    - ", {
      variant: "plugin-args",
      fields: getPluginKindDefinition("forward")!.configSchema,
    });
    expect(result).toContain("addr");
    expect(result).toContain("tag");
    expect(result).not.toContain("edns");
  });
});

it("completes the standalone EDNS filter configuration", () => {
  const editorContext: OxiDnsYamlEditorContext = {
    variant: "plugin-args",
    pluginKind: "edns_filter",
    fields: getPluginKindDefinition("edns_filter")!.configSchema,
  };
  expect(labels("args:\n  ", editorContext)).toEqual([
    "policy",
    "allowlist",
    "denylist",
    "restore_request",
  ]);
  expect(labels("args:\n  policy: ", editorContext)).toEqual([
    "preserve",
    "strip",
    "strip_unknown",
  ]);
});
