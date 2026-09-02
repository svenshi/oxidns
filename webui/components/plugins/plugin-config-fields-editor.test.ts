import { describe, expect, it } from "vitest";

import { matcherPluginDefinitions } from "@/lib/plugin-definitions/matcher";
import { executorPluginDefinitions } from "@/lib/plugin-definitions/executor";
import {
  getLocalizedPluginKindDefinition,
  pluginKindDefinitions,
  type ConfigField,
  type ConfigFieldChild,
} from "@/lib/plugin-definitions";

import {
  createDefaultPluginConfigValues,
  createPluginConfigFormValues,
  formatConfigFieldDefaultValue,
  getConfigFieldExample,
  hasConfiguredAdvancedFields,
  isPluginConfigFormValid,
  mergePluginConfigFormValues,
  omitConfigFieldValues,
  resolveConfigFieldDisplayValue,
  serializePluginConfigValues,
} from "./plugin-config-fields-editor";

describe("schema form config merging", () => {
  it("preserves unknown keys recursively while removing reset known keys", () => {
    const schema: ConfigField[] = [
      { key: "known", label: "Known", type: "text" },
      {
        key: "nested",
        label: "Nested",
        type: "object",
        fields: [{ key: "managed", label: "Managed", type: "number" }],
      },
    ];

    expect(
      mergePluginConfigFormValues(
        schema,
        {
          known: "remove me",
          extension: "keep",
          nested: { managed: 1, extension: "nested keep" },
        },
        { nested: { managed: 2 } },
      ),
    ).toEqual({
      extension: "keep",
      nested: { managed: 2, extension: "nested keep" },
    });
  });

  it("preserves unknown fields in matching array objects", () => {
    const schema: ConfigField[] = [
      {
        key: "jobs",
        label: "Jobs",
        type: "array",
        item: {
          type: "object",
          fields: [
            { key: "name", label: "Name", type: "text" },
            { key: "interval", label: "Interval", type: "duration" },
          ],
        },
      },
    ];

    expect(
      mergePluginConfigFormValues(
        schema,
        {
          jobs: [
            { name: "refresh", interval: "1m", extension: { keep: true } },
          ],
        },
        { jobs: [{ name: "refresh", interval: "5m" }] },
      ),
    ).toEqual({
      jobs: [{ name: "refresh", interval: "5m", extension: { keep: true } }],
    });
  });
});

const timeDefinition = matcherPluginDefinitions.find(
  (definition) => definition.kind === "time",
);

if (!timeDefinition) {
  throw new Error("time matcher definition must exist");
}

const fields = timeDefinition.configSchema;
const periodsField = fields.find((field) => field.key === "periods");

if (!periodsField?.item || periodsField.item.type !== "object") {
  throw new Error("time matcher periods must use an object schema");
}
const periodFields = periodsField.item.fields;

const nftSetDefinition = executorPluginDefinitions.find(
  (definition) => definition.kind === "nftset",
);

if (!nftSetDefinition) {
  throw new Error("nftset executor definition must exist");
}

const qnameDefinition = matcherPluginDefinitions.find(
  (definition) => definition.kind === "qname",
);

if (!qnameDefinition) {
  throw new Error("qname matcher definition must exist");
}

const routerOsDefinitions = executorPluginDefinitions.filter(
  (definition) =>
    definition.kind === "ros_route" || definition.kind === "ros_address_list",
);

const ipSelectorDefinition = executorPluginDefinitions.find(
  (definition) => definition.kind === "ip_selector",
);

if (!ipSelectorDefinition) {
  throw new Error("ip_selector executor definition must exist");
}

const forwardDefinition = executorPluginDefinitions.find(
  (definition) => definition.kind === "forward",
);
const fallbackDefinition = executorPluginDefinitions.find(
  (definition) => definition.kind === "fallback",
);
const httpRequestDefinition = executorPluginDefinitions.find(
  (definition) => definition.kind === "http_request",
);
const preferDefinitions = executorPluginDefinitions.filter(
  (definition) =>
    definition.kind === "prefer_ipv4" || definition.kind === "prefer_ipv6",
);

if (!forwardDefinition || !fallbackDefinition || !httpRequestDefinition) {
  throw new Error(
    "forward, fallback, and http_request executor definitions must exist",
  );
}

describe("client_ip_from_ecs plugin definition", () => {
  it("is registered and localized for both WebUI locales", () => {
    const definition = executorPluginDefinitions.find(
      (candidate) => candidate.kind === "client_ip_from_ecs",
    );

    expect(definition).toMatchObject({
      kind: "client_ip_from_ecs",
      type: "executor",
    });
    expect(definition?.configSchema).toHaveLength(1);
    expect(definition?.configSchema[0]).toMatchObject({
      key: "args",
      type: "array",
      required: false,
      itemOptions: [
        {
          optionKey: "input",
          type: "text",
          label: "输入值",
          example: "127.0.0.1",
        },
      ],
    });
    expect(definition?.configSchema[0]?.default).toBeUndefined();
    const values = createDefaultPluginConfigValues(
      definition?.configSchema ?? [],
    );
    expect(values).toEqual({ args: [] });
    expect(
      serializePluginConfigValues(definition?.configSchema ?? [], values),
    ).toEqual({});
    expect(
      getLocalizedPluginKindDefinition("client_ip_from_ecs", "zh-CN")?.name,
    ).toBe("从 ECS 获取客户端 IP");
    expect(
      getLocalizedPluginKindDefinition("client_ip_from_ecs", "en-US")?.name,
    ).toBe("Client IP From ECS");
  });
});

function collectLegacyPlaceholders(
  fields: ConfigField[],
  parentPath = "",
): string[] {
  const paths: string[] = [];

  const visitChild = (child: ConfigFieldChild, path: string) => {
    if (child.placeholder !== undefined) paths.push(path);
    if (child.type === "object") {
      paths.push(...collectLegacyPlaceholders(child.fields, path));
    }
    if (child.type === "array") {
      if (child.item) visitChild(child.item, `${path}[]`);
      child.itemOptions?.forEach((item) =>
        visitChild(item, `${path}.$${item.optionKey ?? item.type}`),
      );
    }
  };

  fields.forEach((field) => {
    const path = parentPath ? `${parentPath}.${field.key}` : field.key;
    if (field.placeholder !== undefined) paths.push(path);
    if (field.fields) {
      paths.push(...collectLegacyPlaceholders(field.fields, path));
    }
    if (field.item) visitChild(field.item, `${path}[]`);
    field.itemOptions?.forEach((item) =>
      visitChild(item, `${path}.$${item.optionKey ?? item.type}`),
    );
  });

  return paths;
}

function collectUndeclaredDocumentedDefaults(
  fields: ConfigField[],
  parentPath = "",
): string[] {
  const paths: string[] = [];

  fields.forEach((field) => {
    const path = parentPath ? `${parentPath}.${field.key}` : field.key;
    const documentedDefault = field.docs
      ?.split("\n", 1)[0]
      ?.match(/默认值：`([^`]+)`/)?.[1];
    const isDynamicDefault = documentedDefault === "network.outbound.default";

    if (documentedDefault && !isDynamicDefault && field.default === undefined) {
      paths.push(path);
    }
    if (field.fields) {
      paths.push(...collectUndeclaredDocumentedDefaults(field.fields, path));
    }
    if (field.item?.type === "object") {
      paths.push(
        ...collectUndeclaredDocumentedDefaults(field.item.fields, `${path}[]`),
      );
    }
    field.itemOptions?.forEach((item) => {
      if (item.type === "object") {
        paths.push(
          ...collectUndeclaredDocumentedDefaults(
            item.fields,
            `${path}.$${item.optionKey ?? item.type}`,
          ),
        );
      }
    });
  });

  return paths;
}

describe("config field guidance semantics", () => {
  it("uses examples in every built-in schema while retaining legacy fallback", () => {
    const legacyPaths = pluginKindDefinitions.flatMap((definition) =>
      collectLegacyPlaceholders(definition.configSchema).map(
        (path) => `${definition.kind}.${path}`,
      ),
    );

    expect(legacyPaths).toEqual([]);
    expect(
      getConfigFieldExample({
        example: "new-example",
        placeholder: "legacy-example",
      }),
    ).toBe("new-example");
    expect(getConfigFieldExample({ placeholder: "legacy-example" })).toBe(
      "legacy-example",
    );
  });

  it("formats scalar and structured defaults without turning them into examples", () => {
    expect(formatConfigFieldDefaultValue(false)).toBe("false");
    expect(formatConfigFieldDefaultValue(0)).toBe("0");
    expect(formatConfigFieldDefaultValue("")).toBe('""');
    expect(formatConfigFieldDefaultValue(["a", "b"])).toBe('["a","b"]');
    expect(formatConfigFieldDefaultValue({ mode: "safe" })).toBe(
      '{"mode":"safe"}',
    );
  });

  it("declares every literal documented default in the field schema", () => {
    const missingDefaults = pluginKindDefinitions.flatMap((definition) =>
      collectUndeclaredDocumentedDefaults(definition.configSchema).map(
        (path) => `${definition.kind}.${path}`,
      ),
    );

    expect(missingDefaults).toEqual([]);
  });

  it("restores inheritance by removing only the explicit field values", () => {
    const values = { timeout: "10s", enabled: false, tag: "main" };
    const restored = omitConfigFieldValues(values, ["timeout", "enabled"]);

    expect(restored).toEqual({ tag: "main" });
    expect(values).toEqual({ timeout: "10s", enabled: false, tag: "main" });
  });

  it("serializes intentional initial values without materializing runtime defaults", () => {
    const values = createDefaultPluginConfigValues(
      httpRequestDefinition.configSchema,
    );
    const serialized = serializePluginConfigValues(
      httpRequestDefinition.configSchema,
      values,
    );

    expect(values).toMatchObject({ method: "POST" });
    expect(values).not.toHaveProperty("phase");
    expect(values).not.toHaveProperty("async");
    expect(serialized).toEqual({ method: "POST" });
  });
});

if (preferDefinitions.length !== 2) {
  throw new Error("both dual selector executor definitions must exist");
}

describe("time matcher config form", () => {
  it("normalizes legacy weekday aliases to ISO numbers while preserving monthdays", () => {
    const config = {
      timezone: "UTC",
      periods: [
        {
          start: "22:00",
          end: "02:00",
          weekdays: ["fri", "mon"],
          monthdays: [15, 1],
        },
      ],
    };

    const formValues = createPluginConfigFormValues(fields, config);
    const serialized = serializePluginConfigValues(fields, formValues);

    expect(serialized).toEqual({
      ...config,
      periods: [
        {
          ...config.periods[0],
          weekdays: [5, 1],
        },
      ],
    });
    const period = (serialized.periods as Record<string, unknown>[])[0];
    expect(period.weekdays).toEqual([5, 1]);
    expect(period.monthdays).toEqual([15, 1]);
    expect(
      (period.monthdays as unknown[]).every((day) => typeof day === "number"),
    ).toBe(true);
  });

  it("normalizes weekday aliases case-insensitively", () => {
    const formValues = createPluginConfigFormValues(fields, {
      periods: [{ weekdays: ["MON", "Fri"] }],
    });

    expect(serializePluginConfigValues(fields, formValues)).toEqual({
      periods: [{ weekdays: [1, 5] }],
    });
  });

  it("preserves omitted time bounds for existing all-day rules", () => {
    const formValues = createPluginConfigFormValues(fields, {
      periods: [{ weekdays: ["mon", "wed"] }],
    });
    const period = (formValues.periods as Record<string, unknown>[])[0];

    expect(isPluginConfigFormValid(fields, formValues)).toBe(true);
    expect(serializePluginConfigValues(fields, formValues)).toEqual({
      periods: [{ weekdays: [1, 3] }],
    });
    expect(period.start).toBeUndefined();
    expect(period.end).toBeUndefined();
  });

  it("accepts overnight ranges and rejects incomplete or equal ranges", () => {
    const valid = createPluginConfigFormValues(fields, {
      periods: [{ start: "22:00", end: "02:00" }],
    });
    const incomplete = createPluginConfigFormValues(fields, {
      periods: [{ start: "09:00" }],
    });
    const equal = createPluginConfigFormValues(fields, {
      periods: [{ start: "09:00", end: "09:00" }],
    });
    const malformed = createPluginConfigFormValues(fields, {
      periods: [{ start: "9:00", end: "18:00" }],
    });

    expect(isPluginConfigFormValid(fields, valid)).toBe(true);
    expect(isPluginConfigFormValid(fields, incomplete)).toBe(false);
    expect(isPluginConfigFormValid(fields, equal)).toBe(false);
    expect(isPluginConfigFormValid(fields, malformed)).toBe(false);
  });

  it("uses the default range only for newly added periods", () => {
    const newPeriod = createDefaultPluginConfigValues(periodFields);
    const newValues = { periods: [newPeriod] };

    expect(isPluginConfigFormValid(fields, newValues)).toBe(true);
    expect(serializePluginConfigValues(fields, newValues)).toEqual({
      periods: [{ start: "09:00", end: "18:00" }],
    });
  });

  it("rejects an existing empty period without time or calendar conditions", () => {
    const formValues = createPluginConfigFormValues(fields, { periods: [{}] });

    expect(isPluginConfigFormValid(fields, formValues)).toBe(false);
    expect(serializePluginConfigValues(fields, formValues)).toEqual({
      periods: [],
    });
  });

  it("rejects malformed raw YAML period values before normalization", () => {
    const rawYamlValues = { periods: ["bad"] };
    const normalizedValues = createPluginConfigFormValues(
      fields,
      rawYamlValues,
    );

    expect(isPluginConfigFormValid(fields, rawYamlValues)).toBe(false);
    expect(isPluginConfigFormValid(fields, normalizedValues)).toBe(true);
  });
});

describe("optional object config fields", () => {
  it("does not validate required children when an optional object is omitted", () => {
    const formValues = createPluginConfigFormValues(
      nftSetDefinition.configSchema,
      {
        table_family4: "ip",
        table_name4: "mangle",
        set_name4: "dns_v4",
      },
    );

    expect(
      isPluginConfigFormValid(nftSetDefinition.configSchema, formValues),
    ).toBe(true);
    expect(
      serializePluginConfigValues(nftSetDefinition.configSchema, formValues),
    ).toEqual({
      table_family4: "ip",
      table_name4: "mangle",
      set_name4: "dns_v4",
    });
  });

  it("keeps RouterOS TLS disabled in newly created configs", () => {
    expect(routerOsDefinitions).toHaveLength(2);
    for (const definition of routerOsDefinitions) {
      const values = createDefaultPluginConfigValues(definition.configSchema);
      expect(
        serializePluginConfigValues(definition.configSchema, values),
      ).not.toHaveProperty("tls");
    }
  });

  it("preserves an explicitly enabled empty RouterOS TLS object", () => {
    expect(routerOsDefinitions).toHaveLength(2);
    for (const definition of routerOsDefinitions) {
      const values = createPluginConfigFormValues(definition.configSchema, {
        tls: {},
      });

      expect(
        serializePluginConfigValues(definition.configSchema, values),
      ).toHaveProperty("tls", {});
    }
  });
});

describe("optional reference config fields", () => {
  it("omits a cleared dual selector probe executor", () => {
    for (const definition of preferDefinitions) {
      const values = createPluginConfigFormValues(definition.configSchema, {
        probe_executor: "probe_forward",
      });
      values.probe_executor = "";

      expect(
        serializePluginConfigValues(definition.configSchema, values),
      ).not.toHaveProperty("probe_executor");
    }
  });
});

describe("advanced config field visibility", () => {
  const advancedFields = [
    {
      key: "flag",
      label: "Flag",
      type: "switch" as const,
      default: false,
      advanced: true,
    },
    {
      key: "limit",
      label: "Limit",
      type: "number" as const,
      default: 0,
      advanced: true,
    },
    {
      key: "tls",
      label: "TLS",
      type: "object" as const,
      advanced: true,
      fields: [],
    },
  ];

  it("stays collapsed when a new form has no explicitly configured values", () => {
    expect(hasConfiguredAdvancedFields(advancedFields, {})).toBe(false);
  });

  it("does not materialize defaults for unopened advanced objects", () => {
    const values = createDefaultPluginConfigValues(
      ipSelectorDefinition.configSchema,
    );

    expect(values).not.toHaveProperty("cache");
    expect(
      serializePluginConfigValues(ipSelectorDefinition.configSchema, values),
    ).not.toHaveProperty("cache");
  });

  it("does not materialize defaults for unopened advanced scalar fields", () => {
    const cases = [
      [forwardDefinition, "response_selection"],
      [fallbackDefinition, "always_standby"],
    ] as const;

    for (const [definition, key] of cases) {
      const values = createDefaultPluginConfigValues(definition.configSchema);
      expect(values).not.toHaveProperty(key);
      expect(
        serializePluginConfigValues(definition.configSchema, values),
      ).not.toHaveProperty(key);
    }
  });

  it("displays advanced defaults without marking fields as configured", () => {
    expect(resolveConfigFieldDisplayValue(undefined, true)).toBe(true);
    expect(resolveConfigFieldDisplayValue(false, true)).toBe(false);
    expect(resolveConfigFieldDisplayValue(undefined, "balanced")).toBe(
      "balanced",
    );
    expect(resolveConfigFieldDisplayValue("fastest", "balanced")).toBe(
      "fastest",
    );
  });

  it("reveals explicit defaults, false, zero, and empty objects", () => {
    expect(hasConfiguredAdvancedFields(advancedFields, { flag: false })).toBe(
      true,
    );
    expect(hasConfiguredAdvancedFields(advancedFields, { limit: 0 })).toBe(
      true,
    );
    expect(hasConfiguredAdvancedFields(advancedFields, { tls: {} })).toBe(true);
  });
});

describe("item option arrays", () => {
  it("validates already serialized rows without discarding their values", () => {
    const values = { args: ["$blocked", "domain:example.com"] };

    expect(isPluginConfigFormValid(qnameDefinition.configSchema, values)).toBe(
      true,
    );
    expect(
      serializePluginConfigValues(qnameDefinition.configSchema, values),
    ).toEqual(values);
  });
});

describe("time matcher localization", () => {
  it("uses legacy weekday aliases to localize ISO weekday values", () => {
    const localizedDefinition = getLocalizedPluginKindDefinition(
      "time",
      "en-US",
    );
    const localizedPeriods = localizedDefinition?.configSchema.find(
      (field) => field.key === "periods",
    );

    if (!localizedPeriods?.item || localizedPeriods.item.type !== "object") {
      throw new Error(
        "localized time matcher periods must use an object schema",
      );
    }

    const weekdays = localizedPeriods.item.fields.find(
      (field) => field.key === "weekdays",
    );

    expect(
      weekdays?.item && "options" in weekdays.item ? weekdays.item.options : [],
    ).toEqual([
      { label: "Monday", value: 1, aliases: ["mon"] },
      { label: "Tuesday", value: 2, aliases: ["tue"] },
      { label: "Wednesday", value: 3, aliases: ["wed"] },
      { label: "Thursday", value: 4, aliases: ["thu"] },
      { label: "Friday", value: 5, aliases: ["fri"] },
      { label: "Saturday", value: 6, aliases: ["sat"] },
      { label: "Sunday", value: 7, aliases: ["sun"] },
    ]);
  });
});
