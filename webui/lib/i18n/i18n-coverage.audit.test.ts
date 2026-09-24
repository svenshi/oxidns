import { describe, expect, it } from "vitest";

import { pluginKindDefinitions } from "@/lib/plugin-definitions";
import type { ConfigField, ConfigFieldChild } from "@/lib/plugin-definitions";
import { LOCALES, resources } from "@/lib/i18n";

type MessageNode = Record<string, unknown>;

function childKey(field: ConfigFieldChild): string {
  return field.optionKey ?? field.type;
}

function derivedKey(
  spec: NonNullable<
    (typeof pluginKindDefinitions)[number]["metrics"]
  >["derivedCard"] extends (infer T)[] | undefined
    ? T
    : never,
): string {
  if (spec.kind === "latency") return `latency:${spec.prefix}`;
  if (spec.kind === "percent") {
    return `percent:${spec.numerator}/${spec.denominator}`;
  }
  return `percent_of_sum:${spec.numerator}/${spec.terms.join("+")}`;
}

function auditChild(
  field: ConfigFieldChild,
  path: string,
  kindPrefix: string,
  messages: MessageNode,
  missing: string[],
) {
  const localized = messages[path] as MessageNode | undefined;
  if (field.label && typeof localized?.label !== "string") {
    missing.push(`${kindPrefix}.fields.${path}.label`);
  }
  if (field.description && typeof localized?.description !== "string") {
    missing.push(`${kindPrefix}.fields.${path}.description`);
  }
  const example = field.example ?? field.placeholder;
  if (
    example &&
    typeof localized?.example !== "string" &&
    typeof localized?.placeholder !== "string"
  ) {
    missing.push(`${kindPrefix}.fields.${path}.example`);
  }
  if (field.type === "object") {
    auditFields(field.fields, path, kindPrefix, messages, missing);
  }
  if (field.type === "array") {
    if (field.item) {
      auditChild(field.item, `${path}[]`, kindPrefix, messages, missing);
    }
    for (const item of field.itemOptions ?? []) {
      auditChild(
        item,
        `${path}.$${childKey(item)}`,
        kindPrefix,
        messages,
        missing,
      );
    }
  }
}

function auditFields(
  fields: ConfigField[],
  parentPath: string | undefined,
  kindPrefix: string,
  messages: MessageNode,
  missing: string[],
) {
  for (const field of fields) {
    const path = parentPath ? `${parentPath}.${field.key}` : field.key;
    const localized = messages[path] as MessageNode | undefined;
    for (const property of [
      "label",
      "description",
      "example",
      "placeholder",
      "keyPlaceholder",
      "valuePlaceholder",
    ] as const) {
      if (field[property] && typeof localized?.[property] !== "string") {
        missing.push(`${kindPrefix}.fields.${path}.${property}`);
      }
    }
    for (const option of field.options ?? []) {
      const options = localized?.options as MessageNode | undefined;
      const keys = [
        String(option.value),
        ...(option.aliases ?? []).map(String),
      ];
      if (!keys.some((key) => typeof options?.[key] === "string")) {
        missing.push(
          `${kindPrefix}.fields.${path}.options.${String(option.value)}`,
        );
      }
    }
    if (field.fields) {
      auditFields(field.fields, path, kindPrefix, messages, missing);
    }
    if (field.item) {
      auditChild(field.item, `${path}[]`, kindPrefix, messages, missing);
    }
    for (const item of field.itemOptions ?? []) {
      auditChild(
        item,
        `${path}.$${childKey(item)}`,
        kindPrefix,
        messages,
        missing,
      );
    }
  }
}

function auditFieldDocs(
  fields: ConfigField[],
  parentPath: string | undefined,
  kindPrefix: string,
  docs: MessageNode,
  missing: string[],
) {
  for (const field of fields) {
    const path = parentPath ? `${parentPath}.${field.key}` : field.key;
    if (field.docs && typeof docs[path] !== "string") {
      missing.push(`${kindPrefix}.docs.${path}`);
    }
    if (field.fields) {
      auditFieldDocs(field.fields, path, kindPrefix, docs, missing);
    }
    if (field.item?.type === "object") {
      auditFieldDocs(field.item.fields, `${path}[]`, kindPrefix, docs, missing);
    }
    for (const item of field.itemOptions ?? []) {
      if (item.type === "object") {
        auditFieldDocs(
          item.fields,
          `${path}.$${childKey(item)}`,
          kindPrefix,
          docs,
          missing,
        );
      }
    }
  }
}

function auditRequiredFieldDocs(
  fields: ConfigField[],
  parentPath: string | undefined,
  missing: string[],
) {
  for (const field of fields) {
    const path = parentPath ? `${parentPath}.${field.key}` : field.key;
    if (typeof field.docs !== "string" || field.docs.trim().length === 0) {
      missing.push(path);
    }
    if (field.fields) {
      auditRequiredFieldDocs(field.fields, path, missing);
    }
    if (field.item?.type === "object") {
      auditRequiredFieldDocs(field.item.fields, `${path}[]`, missing);
    }
    for (const item of field.itemOptions ?? []) {
      if (item.type === "object") {
        auditRequiredFieldDocs(
          item.fields,
          `${path}.$${childKey(item)}`,
          missing,
        );
      }
    }
  }
}

describe("plugin localization audit", () => {
  for (const locale of LOCALES) {
    it(locale, () => {
      const missing: string[] = [];
      const kinds = resources[locale].plugin.kinds as MessageNode;
      const docs = resources[locale].docs as MessageNode;
      for (const definition of pluginKindDefinitions) {
        const prefix = definition.kind;
        const kind = kinds[prefix] as MessageNode | undefined;
        if (typeof kind?.name !== "string") missing.push(`${prefix}.name`);
        if (typeof kind?.description !== "string") {
          missing.push(`${prefix}.description`);
        }
        auditFields(
          definition.configSchema,
          undefined,
          prefix,
          (kind?.fields as MessageNode | undefined) ?? {},
          missing,
        );
        auditFieldDocs(
          definition.configSchema,
          undefined,
          prefix,
          (docs[prefix] as MessageNode | undefined) ?? {},
          missing,
        );
        const metrics = kind?.metrics as MessageNode | undefined;
        const labels = metrics?.labels as MessageNode | undefined;
        const help = metrics?.help as MessageNode | undefined;
        const derived = metrics?.derived as MessageNode | undefined;
        for (const key of Object.keys(definition.metrics?.metricLabels ?? {})) {
          if (typeof labels?.[key] !== "string") {
            missing.push(`${prefix}.metrics.labels.${key}`);
          }
        }
        for (const key of Object.keys(definition.metrics?.metricHelp ?? {})) {
          if (typeof help?.[key] !== "string") {
            missing.push(`${prefix}.metrics.help.${key}`);
          }
        }
        for (const spec of definition.metrics?.derivedCard ?? []) {
          const key = derivedKey(spec);
          if (typeof derived?.[key] !== "string") {
            missing.push(`${prefix}.metrics.derived.${key}`);
          }
        }
        if (
          definition.quickSetup?.paramPlaceholder &&
          typeof (kind?.quickSetup as MessageNode | undefined)
            ?.paramPlaceholder !== "string"
        ) {
          missing.push(`${prefix}.quickSetup.paramPlaceholder`);
        }
      }

      expect(missing, missing.join("\n")).toEqual([]);
    });
  }

  it("keeps English resources free of Chinese fallback text", () => {
    expect(JSON.stringify(resources["en-US"])).not.toMatch(/[\u4e00-\u9fff]/u);
  });

  it("keeps ros_route field and metric documentation complete", () => {
    const definition = pluginKindDefinitions.find(
      (candidate) => candidate.kind === "ros_route",
    );
    expect(definition).toBeDefined();
    if (!definition) return;

    const missingFieldDocs: string[] = [];
    auditRequiredFieldDocs(
      definition.configSchema,
      undefined,
      missingFieldDocs,
    );
    expect(missingFieldDocs, missingFieldDocs.join("\n")).toEqual([]);

    const metricLabels = Object.keys(
      definition.metrics?.metricLabels ?? {},
    ).sort();
    const metricHelp = Object.keys(definition.metrics?.metricHelp ?? {}).sort();
    expect(metricHelp).toEqual(metricLabels);
  });
});
