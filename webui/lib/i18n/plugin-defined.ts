import type {
  ConfigField,
  ConfigFieldChild,
  PluginKindDefinition,
} from "@/lib/plugin-definitions";
import type { PluginMetricsDef } from "@/lib/plugin-definitions/shared";
import type { PluginStatus, PluginType } from "@/lib/types";
import { DEFAULT_LOCALE, resources, type Locale } from "@/lib/i18n";

type KindMessages = {
  name?: string;
  description?: string;
  fields?: FieldMessages;
  metrics?: {
    labels?: Record<string, string>;
    help?: Record<string, string>;
    derived?: Record<string, string>;
  };
  quickSetup?: {
    paramPlaceholder?: string;
  };
};
type PluginMessages = {
  pluginTypes: {
    labels: Record<PluginType, string>;
    descriptions: Record<PluginType, string>;
    statuses: Record<PluginStatus, string>;
  };
  kinds: Record<string, KindMessages>;
};
type FieldMessages = Record<
  string,
  Partial<
    Record<
      | "label"
      | "description"
      | "example"
      | "placeholder"
      | "keyPlaceholder"
      | "valuePlaceholder",
      string
    >
  > & {
    options?: Record<string, string>;
  }
>;

export function getLocalizedPluginKindDefinition(
  definition: PluginKindDefinition,
  locale: Locale,
): PluginKindDefinition {
  const kindMessages = getKindMessages(locale, definition.kind);
  const fallbackMessages = getKindMessages(DEFAULT_LOCALE, definition.kind);

  return {
    ...definition,
    name: kindMessages?.name ?? fallbackMessages?.name ?? definition.name,
    description:
      kindMessages?.description ??
      fallbackMessages?.description ??
      definition.description,
    configSchema: localizeFields(
      definition.kind,
      definition.configSchema,
      locale,
    ),
    metrics: localizeMetrics(definition.kind, definition.metrics, locale),
    quickSetup: definition.quickSetup
      ? {
          ...definition.quickSetup,
          paramPlaceholder:
            kindMessages?.quickSetup?.paramPlaceholder ??
            fallbackMessages?.quickSetup?.paramPlaceholder ??
            definition.quickSetup.paramPlaceholder,
        }
      : definition.quickSetup,
  };
}

export function pluginTypeLabel(type: PluginType, locale: Locale): string {
  return getPluginMessages(locale).pluginTypes.labels[type];
}

export function pluginTypeDescription(
  type: PluginType,
  locale: Locale,
): string {
  return getPluginMessages(locale).pluginTypes.descriptions[type];
}

export function pluginStatusLabel(
  status: PluginStatus,
  locale: Locale,
): string {
  return getPluginMessages(locale).pluginTypes.statuses[status];
}

export function getPluginSearchText(
  definition: PluginKindDefinition,
  locales: Locale[],
): string {
  return locales
    .map((locale) => {
      const localized = getLocalizedPluginKindDefinition(definition, locale);
      return [
        localized.kind,
        localized.name,
        localized.description,
        pluginTypeLabel(localized.type, locale),
        collectFieldSearchText(localized.configSchema),
        collectMetricSearchText(localized.metrics),
      ].join(" ");
    })
    .join(" ");
}

function localizeMetrics(
  kind: string,
  metrics: PluginMetricsDef | undefined,
  locale: Locale,
): PluginMetricsDef | undefined {
  if (!metrics) return metrics;
  const messages = getKindMessages(locale, kind)?.metrics;
  const fallback = getKindMessages(DEFAULT_LOCALE, kind)?.metrics;
  return {
    ...metrics,
    metricLabels: localizeRecord(
      metrics.metricLabels,
      messages?.labels,
      fallback?.labels,
    ),
    metricHelp: localizeRecord(
      metrics.metricHelp,
      messages?.help,
      fallback?.help,
    ),
    derivedCard: metrics.derivedCard?.map((spec) => ({
      ...spec,
      label:
        messages?.derived?.[derivedMetricKey(spec)] ??
        fallback?.derived?.[derivedMetricKey(spec)] ??
        spec.label,
    })),
  };
}

function localizeFields(
  kind: string,
  fields: ConfigField[],
  locale: Locale,
  parentPath?: string,
): ConfigField[] {
  const fieldMessages = getFieldMessages(locale, kind);
  const fallbackFieldMessages = getFieldMessages(DEFAULT_LOCALE, kind);

  return fields.map((field) => {
    const fieldPath = parentPath ? `${parentPath}.${field.key}` : field.key;
    const localized = applyFieldMessages(
      field,
      fieldMessages[fieldPath],
      fallbackFieldMessages[fieldPath],
    );

    return {
      ...localized,
      docs:
        getDocsMessage(locale, kind, fieldPath) ??
        getDocsMessage(DEFAULT_LOCALE, kind, fieldPath) ??
        field.docs,
      fields: field.fields
        ? localizeFields(kind, field.fields, locale, fieldPath)
        : field.fields,
      item: field.item
        ? localizeChildField(kind, field.item, locale, `${fieldPath}[]`)
        : field.item,
      itemOptions: field.itemOptions?.map((item) =>
        localizeChildField(
          kind,
          item,
          locale,
          `${fieldPath}.$${getChildOptionKey(item)}`,
        ),
      ),
    };
  });
}

function localizeChildField(
  kind: string,
  field: ConfigFieldChild,
  locale: Locale,
  fieldPath: string,
): ConfigFieldChild {
  const fieldMessages = getFieldMessages(locale, kind);
  const fallbackFieldMessages = getFieldMessages(DEFAULT_LOCALE, kind);
  const localized = applyFieldMessages(
    field,
    fieldMessages[fieldPath],
    fallbackFieldMessages[fieldPath],
  );

  if (localized.type === "object") {
    return {
      ...localized,
      fields: localizeFields(kind, localized.fields, locale, fieldPath),
    };
  }
  if (localized.type === "array") {
    return {
      ...localized,
      item: localized.item
        ? localizeChildField(kind, localized.item, locale, `${fieldPath}[]`)
        : localized.item,
      itemOptions: localized.itemOptions?.map((item) =>
        localizeChildField(
          kind,
          item,
          locale,
          `${fieldPath}.$${getChildOptionKey(item)}`,
        ),
      ),
    };
  }
  return localized;
}

function applyFieldMessages<T extends ConfigField | ConfigFieldChild>(
  field: T,
  messages: FieldMessages[string] | undefined,
  fallback: FieldMessages[string] | undefined,
): T {
  const next = { ...field } as T;
  for (const key of [
    "label",
    "description",
    "example",
    "placeholder",
    "keyPlaceholder",
    "valuePlaceholder",
  ] as const) {
    const value = messages?.[key] ?? fallback?.[key];
    if (value) {
      (next as Record<string, unknown>)[key] = value;
    }
  }
  if ("options" in next && next.options) {
    next.options = next.options.map((option) => ({
      ...option,
      label: localizeOptionLabel(option, messages, fallback),
    }));
  }
  return next;
}

function localizeOptionLabel(
  option: NonNullable<ConfigField["options"]>[number],
  messages: FieldMessages[string] | undefined,
  fallback: FieldMessages[string] | undefined,
) {
  const keys = [String(option.value), ...(option.aliases ?? []).map(String)];
  return (
    keys.map((key) => messages?.options?.[key]).find(Boolean) ??
    keys.map((key) => fallback?.options?.[key]).find(Boolean) ??
    option.label
  );
}

function localizeRecord(
  source: Record<string, string> | undefined,
  messages: Record<string, string> | undefined,
  fallback: Record<string, string> | undefined,
): Record<string, string> | undefined {
  if (!source) return source;
  return Object.fromEntries(
    Object.entries(source).map(([key, value]) => [
      key,
      messages?.[key] ?? fallback?.[key] ?? value,
    ]),
  );
}

function getPluginMessages(locale: Locale): PluginMessages {
  return resources[locale].plugin as PluginMessages;
}

function getKindMessages(locale: Locale, kind: string) {
  return getPluginMessages(locale).kinds[kind];
}

function getFieldMessages(locale: Locale, kind: string): FieldMessages {
  return (getKindMessages(locale, kind)?.fields ?? {}) as FieldMessages;
}

function getDocsMessage(
  locale: Locale,
  kind: string,
  fieldPath: string,
): string | undefined {
  const docs = resources[locale].docs as Record<string, Record<string, string>>;
  return docs[kind]?.[fieldPath];
}

function getChildOptionKey(item: ConfigFieldChild): string {
  return item.optionKey ?? item.type;
}

function derivedMetricKey(
  spec: NonNullable<PluginMetricsDef["derivedCard"]>[number],
) {
  if (spec.kind === "latency") return `latency:${spec.prefix}`;
  if (spec.kind === "percent") {
    return `percent:${spec.numerator}/${spec.denominator}`;
  }
  return `percent_of_sum:${spec.numerator}/${spec.terms.join("+")}`;
}

function collectFieldSearchText(fields: ConfigField[]): string {
  return fields.map(collectSingleFieldSearchText).join(" ");
}

function collectSingleFieldSearchText(field: ConfigField): string {
  return [
    field.key,
    field.label,
    field.description,
    field.example,
    field.placeholder,
    field.docs,
    field.options?.map((option) => option.label).join(" "),
    field.fields ? collectFieldSearchText(field.fields) : undefined,
    field.item ? collectChildFieldSearchText(field.item) : undefined,
    field.itemOptions?.map(collectChildFieldSearchText).join(" "),
  ]
    .filter(Boolean)
    .join(" ");
}

function collectChildFieldSearchText(field: ConfigFieldChild): string {
  return [
    field.label,
    field.description,
    field.example,
    field.placeholder,
    "fields" in field ? collectFieldSearchText(field.fields) : undefined,
    "item" in field && field.item
      ? collectChildFieldSearchText(field.item)
      : undefined,
    "itemOptions" in field && field.itemOptions
      ? field.itemOptions.map(collectChildFieldSearchText).join(" ")
      : undefined,
  ]
    .filter(Boolean)
    .join(" ");
}

function collectMetricSearchText(
  metrics: PluginMetricsDef | undefined,
): string {
  if (!metrics) return "";
  return [
    Object.keys(metrics.metricLabels ?? {}).join(" "),
    Object.values(metrics.metricLabels ?? {}).join(" "),
    Object.values(metrics.metricHelp ?? {}).join(" "),
    metrics.derivedCard?.map((spec) => spec.label).join(" "),
  ]
    .filter(Boolean)
    .join(" ");
}
