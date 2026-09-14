import type { PluginType } from "../types";

/**
 * Declarative spec for a derived metric shown on the plugin card.
 * - latency: averages `{prefix}_latency_sum_ms / {prefix}_latency_count`
 * - percent: shows `numerator / denominator` as a percentage
 * - percent_of_sum: shows `numerator / sum(terms)` as a percentage
 */
export type DerivedMetricSpec =
  | { kind: "latency"; prefix: string; label: string }
  | { kind: "percent"; numerator: string; denominator: string; label: string }
  | {
      kind: "percent_of_sum";
      numerator: string;
      terms: [string, ...string[]];
      label: string;
    };

export interface PluginMetricsDef {
  /** Prometheus metric name → Chinese display label for this plugin's metrics. */
  metricLabels?: Record<string, string>;
  /** Prometheus metric name → Chinese description shown in the detail panel (overrides backend HELP). */
  metricHelp?: Record<string, string>;
  /** Ordered metric names surfaced on the plugin card (up to 6 shown). */
  cardPriority?: string[];
  /** Derived metrics prepended to card display before raw metric values. */
  derivedCard?: DerivedMetricSpec[];
}

// Add new plugin kinds here first. The web UI catalog, create dialog, cards, and
// detail drawer all resolve their display metadata from these definitions.
export interface PluginKindDefinition {
  kind: string;
  type: PluginType;
  name: string;
  description: string;
  icon: string;
  configSchema: ConfigField[];
  /** Metrics emitted by this plugin kind: labels, card priority, and derived metrics. */
  metrics?: PluginMetricsDef;
  /**
   * Marks this plugin kind as usable inline inside a sequence rule via
   * `quick_setup` syntax. Drives the "快捷" mode in the sequence canvas
   * (sequence-composer.tsx) so the user can write e.g. `qname $domain_set`
   * directly without first defining a named plugin instance.
   *
   * Leave undefined for kinds whose `fn quick_setup` is not implemented in the
   * Rust backend — those can only be referenced via a normal plugin tag.
   */
  quickSetup?: {
    /**
     * Placeholder shown in the param input when no value is set yet.
     * For example: "domain:example.com 或 $domain_set" for qname.
     */
    paramPlaceholder?: string;
    /**
     * If the param is typically a `$tag` reference to another plugin, list the
     * plugin type(s) here. The composer will render a reference picker
     * limited to those types instead of a free-text input. Leave empty for
     * builtins with no param (e.g. `has_resp`, `true_matcher`) or pure
     * scalar params (e.g. `random 0.1`).
     */
    paramReferenceTypes?: PluginType[];
  };
}
export type ConfigFieldType =
  | "text"
  | "password"
  | "time"
  | "number"
  | "select"
  | "textarea"
  | "switch"
  | "array"
  | "object"
  | "duration"
  | "json"
  | "record"
  | "reference";

export interface ConfigTimeRangeGroup {
  id: string;
  role: "start" | "end";
  defaultValue: string;
}

export interface ConfigField {
  key: string;
  label: string;
  type: ConfigFieldType;
  /** Machine-readable example shown as input guidance, never as an effective value. */
  example?: string;
  /** @deprecated Use `example`; retained for third-party schema compatibility. */
  placeholder?: string;
  description?: string;
  docs?: string;
  required?: boolean;
  /** Form value intentionally written for a new config, distinct from a runtime default. */
  initialValue?: unknown;
  default?: unknown;
  /** Render this field inside the collapsed advanced-settings section. */
  advanced?: boolean;
  options?: {
    label: string;
    value: string | number;
    /** Additional legacy values accepted when loading form data. */
    aliases?: (string | number)[];
  }[];
  dynamicOptions?: "outboundProfiles";
  referenceTypes?: PluginType[];
  referencePlugins?: string[];
  referencePrefix?: "$" | "";
  allowInvert?: boolean;
  asArray?: boolean;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
  item?: ConfigFieldChild;
  itemOptions?: ConfigFieldChild[];
  /** Render a finite array as direct multi-choice controls instead of add/remove rows. */
  arrayPresentation?: "checklist" | "weekday-chips" | "calendar-grid";
  /** Group two sibling time fields into one optional start/end range editor. */
  timeRange?: ConfigTimeRangeGroup;
  fields?: ConfigField[];
  /** Preserve an enabled optional object even when all of its child fields are empty. */
  preserveEmptyObject?: boolean;
  summaryFields?: string[];
  /** @deprecated Settings rows now give every control the full value column. */
  fullWidth?: boolean;
}
export type ConfigFieldChild =
  | ({
      type: Exclude<ConfigFieldType, "array" | "object">;
    } & Omit<
      ConfigField,
      | "key"
      | "type"
      | "item"
      | "itemOptions"
      | "fields"
      | "label"
      | "required"
      | "summaryFields"
    > & {
        optionKey?: string;
        label?: string;
      })
  | {
      type: "array";
      optionKey?: string;
      label?: string;
      example?: string;
      /** @deprecated Use `example`; retained for third-party schema compatibility. */
      placeholder?: string;
      description?: string;
      item?: ConfigFieldChild;
      itemOptions?: ConfigFieldChild[];
    }
  | {
      type: "object";
      optionKey?: string;
      label?: string;
      example?: string;
      /** @deprecated Use `example`; retained for third-party schema compatibility. */
      placeholder?: string;
      description?: string;
      fields: ConfigField[];
      summaryFields?: string[];
    };
export type ConfigArrayItem = ConfigFieldChild;
export const executorRef = (
  key: string,
  label: string,
  required = true,
  referencePlugins?: string[],
  description?: string,
): ConfigField => ({
  key,
  label,
  type: "reference",
  required,
  referenceTypes: ["executor"],
  referencePlugins,
  description,
});
export const matcherListField = (
  description = "每行一个 matcher 表达式，支持 $tag、快捷表达式和 ! 取反",
): ConfigField => ({
  key: "args",
  label: "匹配表达式",
  type: "array",
  required: true,
  example: "$match_tag\nqname domain:example.com\n!$blocked",
  description,
  itemOptions: [
    {
      optionKey: "matcher_ref",
      type: "reference",
      label: "引用 matcher",
      referenceTypes: ["matcher"],
      referencePrefix: "$",
      allowInvert: true,
      example: "match_tag",
    },
    {
      optionKey: "input",
      type: "text",
      label: "输入值",
      example: "qname domain:example.com",
    },
  ],
});
export const stringArrayField = (
  key: string,
  label: string,
  example: string,
  required = false,
  description = "每行一项",
  item?: ConfigFieldChild,
  itemOptions?: ConfigFieldChild[],
): ConfigField => ({
  key,
  label,
  type: "array",
  required,
  example,
  description,
  item: itemOptions ? item : (item ?? inputArrayItem(example.split("\n")[0])),
  itemOptions,
});
export const inputArrayItem = (example: string): ConfigFieldChild => ({
  optionKey: "input",
  type: "text",
  label: "输入值",
  example,
});
export const providerReferenceArrayItem = (
  example: string,
): ConfigFieldChild => ({
  optionKey: "provider_ref",
  type: "reference",
  label: "引用 provider",
  referenceTypes: ["provider"],
  referencePrefix: "$",
  example,
});
export const executorReferenceArrayItem = (
  example: string,
): ConfigFieldChild => ({
  optionKey: "executor_ref",
  type: "reference",
  label: "引用 executor",
  referenceTypes: ["executor"],
  referencePrefix: "$",
  example,
});
export const nftSetTargetFields: ConfigField[] = [
  {
    key: "table_family",
    label: "表 Family",
    type: "text",
    example: "ip",
    required: true,
  },
  {
    key: "table_name",
    label: "表名",
    type: "text",
    example: "mangle",
    required: true,
  },
  {
    key: "set_name",
    label: "Set 名称",
    type: "text",
    example: "dns_v4",
    required: true,
  },
  {
    key: "mask",
    label: "前缀长度",
    type: "number",
    example: "24",
    advanced: true,
  },
];
