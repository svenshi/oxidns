"use client";

import { PluginReferencePicker } from "@/components/plugins/plugin-reference-picker";
import { AdvancedSettingsSection } from "@/components/plugins/advanced-settings-section";
import { ConfigProvider, TimePicker } from "antd";
import enUS from "antd/locale/en_US";
import zhCN from "antd/locale/zh_CN";
import dayjs from "dayjs";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useAppStore } from "@/lib/store";
import type { ConfigField, ConfigFieldChild } from "@/lib/plugin-definitions";
import type { PluginInstance, PluginType } from "@/lib/types";
import { cn } from "@/lib/utils";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import { ChevronDown, Info, Minus, Plus, RotateCcw, X } from "lucide-react";
import {
  Fragment,
  useEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";

type ArrayItemSyntax = "value" | "plugin" | "quick" | "domain";

interface ArrayItemValue {
  id: string;
  syntax: ArrayItemSyntax;
  value: string;
  invert?: boolean;
  referenceTypes?: PluginType[];
}

interface SchemaArrayOptionValue {
  id: string;
  optionKey: string;
  value: unknown;
}

export function shouldShowSchemaArrayEntryHeader(
  field: Pick<ConfigField, "itemOptions">,
  child: ConfigFieldChild,
) {
  return (
    child.type === "object" ||
    child.type === "array" ||
    Boolean(field.itemOptions)
  );
}

interface RecordItemValue {
  id: string;
  key: string;
  value: string;
}

interface PluginConfigFieldsEditorProps {
  fields: ConfigField[];
  plugins: PluginInstance[];
  values: Record<string, unknown>;
  configuredValues?: Record<string, unknown>;
  onChange: (values: Record<string, unknown>) => void;
  defaultArrayObjectCollapsed?: boolean;
  readOnly?: boolean;
}

const ARRAY_SYNTAX_KEYS: Record<ArrayItemSyntax, string> = {
  value: WEBUI.plugins.arraySyntaxValue,
  plugin: WEBUI.plugins.arraySyntaxPlugin,
  quick: WEBUI.plugins.arraySyntaxQuick,
  domain: WEBUI.plugins.arraySyntaxDomain,
};

const OPTIONAL_SELECT_VALUE = "__oxidns_unset__";
const OBJECT_PRESENCE_KEY = "__oxidns_object_present__";

function isPresentOptionalObject(value: unknown): boolean {
  return (
    !!value &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    (value as Record<string, unknown>)[OBJECT_PRESENCE_KEY] === true
  );
}

function InvertCheckbox({
  checked,
  disabled,
  onCheckedChange,
}: {
  checked: boolean;
  disabled: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  const { t } = useI18n();
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          className={`flex h-8 w-6 shrink-0 items-center justify-center rounded-md border font-mono text-sm font-bold leading-none ${
            checked
              ? "border-primary bg-primary text-primary-foreground"
              : "border-input bg-background text-transparent"
          } disabled:cursor-not-allowed disabled:opacity-50`}
          aria-label={t(WEBUI.plugins.invertMatch)}
          disabled={disabled}
          onClick={() => onCheckedChange(!checked)}
        >
          !
        </button>
      </TooltipTrigger>
      <TooltipContent sideOffset={6}>
        {t(WEBUI.plugins.invertMatch)}
      </TooltipContent>
    </Tooltip>
  );
}

export function createDefaultPluginConfigValues(fields: ConfigField[]) {
  const defaults: Record<string, unknown> = {};
  fields.forEach((field) => {
    if (field.advanced) {
      return;
    }
    if (field.initialValue !== undefined) {
      defaults[field.key] = field.initialValue;
    } else if (field.type === "array") {
      defaults[field.key] = [];
    } else if (field.type === "time" && field.timeRange) {
      defaults[field.key] = field.timeRange.defaultValue;
    } else if (field.type === "object" && field.fields) {
      const objectDefaults = createDefaultPluginConfigValues(field.fields);
      if (field.preserveEmptyObject) {
        objectDefaults[OBJECT_PRESENCE_KEY] = false;
      }
      defaults[field.key] = objectDefaults;
    } else if (field.type === "record") {
      defaults[field.key] = [];
    } else if (field.type === "json") {
      defaults[field.key] = "";
    }
  });
  return defaults;
}

export function resolveConfigFieldDisplayValue(
  value: unknown,
  defaultValue: unknown,
) {
  return value === undefined ? defaultValue : value;
}

export function getConfigFieldExample(
  field: Pick<ConfigField, "example" | "placeholder">,
) {
  return field.example ?? field.placeholder;
}

export function formatConfigFieldDefaultValue(value: unknown): string {
  if (typeof value === "string") return value || '""';
  if (value === undefined) return "";
  if (value === null) return "null";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

export function omitConfigFieldValues(
  values: Record<string, unknown>,
  keys: string[],
) {
  const nextValues = { ...values };
  keys.forEach((key) => delete nextValues[key]);
  return nextValues;
}

function formatConfigFieldDefaultSummary(
  field: ConfigField,
  t: (key: string, params?: Record<string, string | number>) => string,
) {
  if (Array.isArray(field.default)) {
    return t(WEBUI.common.itemCount, { count: field.default.length });
  }
  if (typeof field.default === "boolean") {
    return t(field.default ? WEBUI.common.enabled : WEBUI.common.disabled);
  }
  return formatConfigFieldDefaultValue(field.default);
}

function ConfigFieldResetButton({
  field,
  configured,
  onReset,
  readOnly,
}: {
  field: ConfigField;
  configured: boolean;
  onReset: () => void;
  readOnly: boolean;
}) {
  const { t } = useI18n();
  const hasDefault = field.default !== undefined;
  const canReset = configured && !readOnly && (hasDefault || !field.required);
  if (!canReset) return null;

  const label = hasDefault
    ? t(WEBUI.plugins.restoreDefaultValue, {
        value: formatConfigFieldDefaultSummary(field, t),
      })
    : t(WEBUI.plugins.clearConfigValue);

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          className="shrink-0 text-muted-foreground opacity-60 transition-opacity hover:text-primary focus-visible:opacity-100 sm:opacity-0 sm:group-hover/field:opacity-100"
          aria-label={label}
          onClick={onReset}
        >
          <RotateCcw />
        </Button>
      </TooltipTrigger>
      <TooltipContent sideOffset={6}>{label}</TooltipContent>
    </Tooltip>
  );
}

function ConfigFieldReadValue({
  field,
  value,
  configured,
}: {
  field: ConfigField;
  value: unknown;
  configured: boolean;
}) {
  const { t } = useI18n();
  const configModel = useAppStore((state) => state.configModel);
  const displayValue = resolveConfigFieldDisplayValue(value, field.default);
  const inherited = !configured && field.default !== undefined;

  if (isEmptyConfigValue(displayValue)) {
    return (
      <span className="block py-1 text-sm text-muted-foreground">
        {t(WEBUI.common.unconfigured)}
      </span>
    );
  }

  let renderedValue: ReactNode;
  if (field.type === "switch" && typeof displayValue === "boolean") {
    renderedValue = t(
      displayValue ? WEBUI.common.enabled : WEBUI.common.disabled,
    );
  } else if (field.type === "select") {
    const option = resolveSelectOptions(field, configModel).find(
      (candidate) => String(candidate.value) === String(displayValue),
    );
    renderedValue =
      option?.label ?? formatConfigFieldDefaultValue(displayValue);
  } else if (field.type === "password") {
    renderedValue = "••••••••";
  } else {
    renderedValue = formatConfigFieldDefaultValue(displayValue);
  }

  return (
    <code
      className={cn(
        "block min-w-0 whitespace-pre-wrap break-all py-1 font-mono text-sm leading-5",
        inherited ? "text-muted-foreground" : "text-foreground",
      )}
    >
      {renderedValue}
    </code>
  );
}

function ConfigFieldExample({ field }: { field: ConfigField }) {
  const { t } = useI18n();
  const example = getConfigFieldExample(field);
  if (!example || (field.type !== "time" && field.type !== "reference")) {
    return null;
  }

  return (
    <FieldDescription className="text-config-example text-xs leading-4">
      {t(WEBUI.plugins.configExampleLabel)}:
      <code className="ml-1 whitespace-pre-wrap break-all font-mono">
        {example}
      </code>
    </FieldDescription>
  );
}

export function createPluginConfigFormValues(
  fields: ConfigField[],
  config: Record<string, unknown>,
) {
  const values = createDefaultPluginConfigValues(fields);

  fields.forEach((field) => {
    const value = config[field.key];
    if (value === undefined) {
      if (field.type === "time" && field.timeRange) {
        delete values[field.key];
      }
      return;
    }

    if (field.type === "array") {
      values[field.key] = normalizeArrayFieldValue(value, field);
    } else if (field.type === "object" && field.fields) {
      const objectValues =
        value && typeof value === "object" && !Array.isArray(value)
          ? createPluginConfigFormValues(
              field.fields,
              value as Record<string, unknown>,
            )
          : createDefaultPluginConfigValues(field.fields);
      if (field.preserveEmptyObject) {
        objectValues[OBJECT_PRESENCE_KEY] = true;
      }
      values[field.key] = objectValues;
    } else if (field.type === "record") {
      values[field.key] = normalizeRecordValue(value);
    } else if (field.type === "json") {
      values[field.key] =
        typeof value === "string" ? value : JSON.stringify(value, null, 2);
    } else {
      values[field.key] = value;
    }
  });

  return values;
}

export function serializePluginConfigValues(
  fields: ConfigField[],
  values: Record<string, unknown>,
) {
  const config: Record<string, unknown> = {};

  fields.forEach((field) => {
    const value = values[field.key];
    if (field.type === "array" && Array.isArray(value)) {
      const serialized = serializeArrayFieldValue(value, field);
      if (serialized.length > 0 || field.required)
        config[field.key] = serialized;
    } else if (field.type === "array" && typeof value === "string") {
      const serialized = value
        .split("\n")
        .map((v) => v.trim())
        .filter(Boolean);
      if (serialized.length > 0 || field.required)
        config[field.key] = serialized;
    } else if (
      field.type === "json" &&
      typeof value === "string" &&
      value.trim()
    ) {
      try {
        config[field.key] = JSON.parse(value);
      } catch {
        config[field.key] = value;
      }
    } else if (field.type === "object" && field.fields) {
      const serialized =
        value && typeof value === "object" && !Array.isArray(value)
          ? serializePluginConfigValues(
              field.fields,
              value as Record<string, unknown>,
            )
          : {};
      if (
        !isEmptyConfigValue(serialized) ||
        field.required ||
        (field.preserveEmptyObject && isPresentOptionalObject(value))
      ) {
        config[field.key] = serialized;
      }
    } else if (field.type === "record" && Array.isArray(value)) {
      const serialized = serializeRecordValue(value as RecordItemValue[]);
      if (!isEmptyConfigValue(serialized) || field.required) {
        config[field.key] = serialized;
      }
    } else if (field.asArray && value !== undefined && value !== "") {
      config[field.key] = [value];
    } else if (value !== undefined && value !== "") {
      config[field.key] = value;
    }
  });

  return config;
}

/**
 * Merge values owned by a schema-driven form into the full plugin config.
 * Unknown keys are intentionally retained; omitting a known key still means
 * that the operator reset it and removes it from the serialized config.
 */
export function mergePluginConfigFormValues(
  fields: ConfigField[],
  base: Record<string, unknown>,
  serialized: Record<string, unknown>,
) {
  const merged: Record<string, unknown> = { ...base };
  fields.forEach((field) => {
    if (!Object.prototype.hasOwnProperty.call(serialized, field.key)) {
      delete merged[field.key];
      return;
    }
    merged[field.key] = mergeKnownFieldValue(
      field,
      base[field.key],
      serialized[field.key],
    );
  });
  return merged;
}

function mergeKnownFieldValue(
  field: ConfigField,
  base: unknown,
  serialized: unknown,
): unknown {
  if (
    field.type === "object" &&
    field.fields &&
    isPlainObject(base) &&
    isPlainObject(serialized)
  ) {
    return mergePluginConfigFormValues(field.fields, base, serialized);
  }

  if (
    field.type === "array" &&
    Array.isArray(base) &&
    Array.isArray(serialized)
  ) {
    return serialized.map((entry, index) => {
      if (!isPlainObject(entry)) return entry;
      const itemFields = arrayObjectFields(field, entry);
      if (!itemFields) return entry;
      const baseEntry = findMatchingArrayObject(base, entry, index);
      return isPlainObject(baseEntry)
        ? mergePluginConfigFormValues(itemFields, baseEntry, entry)
        : entry;
    });
  }

  return serialized;
}

function arrayObjectFields(field: ConfigField, value: Record<string, unknown>) {
  const candidates = [field.item, ...(field.itemOptions ?? [])].filter(
    (item): item is Extract<NonNullable<typeof item>, { type: "object" }> =>
      item?.type === "object",
  );
  if (candidates.length === 0) return null;
  return candidates
    .map((candidate) => ({
      fields: candidate.fields,
      score: candidate.fields.filter((child) => child.key in value).length,
    }))
    .sort((left, right) => right.score - left.score)[0].fields;
}

function findMatchingArrayObject(
  base: unknown[],
  value: Record<string, unknown>,
  index: number,
) {
  for (const identityKey of ["name", "tag", "id", "key"]) {
    const identity = value[identityKey];
    if (identity === undefined) continue;
    const match = base.find(
      (candidate) =>
        isPlainObject(candidate) && candidate[identityKey] === identity,
    );
    if (match) return match;
  }
  return base[index];
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}

export function isPluginConfigFormValid(
  fields: ConfigField[],
  values: Record<string, unknown>,
): boolean {
  return fields.every((field) => {
    const value = values[field.key];
    if (field.required) {
      if (
        Array.isArray(value) &&
        serializeArrayFieldValue(value, field).length === 0
      ) {
        return false;
      }
      if (value === undefined || value === "") return false;
    }

    if (field.type === "time" && !isEmptyConfigValue(value)) {
      if (typeof value !== "string" || !isValidTimeValue(value)) return false;
    }

    if (field.timeRange?.role === "start") {
      const endField = findTimeRangePair(fields, field);
      if (!endField) return false;
      const endValue = values[endField.key];
      const hasStart = !isEmptyConfigValue(value);
      const hasEnd = !isEmptyConfigValue(endValue);
      if (hasStart !== hasEnd) return false;
      if (hasStart && value === endValue) return false;
    }

    if (
      field.type === "object" &&
      field.fields &&
      value &&
      typeof value === "object" &&
      !Array.isArray(value)
    ) {
      if (field.preserveEmptyObject && !isPresentOptionalObject(value)) {
        return true;
      }
      if (!field.required && isEmptyConfigValue(value)) return true;
      return isPluginConfigFormValid(
        field.fields,
        value as Record<string, unknown>,
      );
    }

    if (
      field.type === "array" &&
      field.item?.type === "object" &&
      Array.isArray(value)
    ) {
      const itemFields = field.item.fields;
      return value.every(
        (entry) =>
          entry !== null &&
          typeof entry === "object" &&
          !Array.isArray(entry) &&
          isPluginConfigFormValid(itemFields, entry as Record<string, unknown>),
      );
    }

    return true;
  });
}

const TIME_VALUE_PATTERN = /^(?:[01]\d|2[0-3]):[0-5]\d$/;

function isValidTimeValue(value: string) {
  return TIME_VALUE_PATTERN.test(value);
}

function findTimeRangePair(fields: ConfigField[], field: ConfigField) {
  if (!field.timeRange) return undefined;
  const group = field.timeRange;
  const pairRole = group.role === "start" ? "end" : "start";
  return fields.find(
    (candidate) =>
      candidate.timeRange?.id === group.id &&
      candidate.timeRange?.role === pairRole,
  );
}

export function PluginConfigFieldsEditor({
  fields,
  plugins,
  values,
  configuredValues = values,
  onChange,
  defaultArrayObjectCollapsed = false,
  readOnly = false,
}: PluginConfigFieldsEditorProps) {
  const { t } = useI18n();
  const regularFields = fields.filter((field) => !field.advanced);
  const advancedFields = fields.filter((field) => field.advanced);
  const hasConfiguredAdvancedValue = hasConfiguredAdvancedFields(
    advancedFields,
    configuredValues,
  );
  const updateConfig = (key: string, value: unknown) => {
    onChange({ ...values, [key]: value });
  };
  const resetConfig = (key: string) => {
    onChange(omitConfigFieldValues(values, [key]));
  };

  if (fields.length === 0) {
    return (
      <div className="rounded-lg border border-dashed p-4 text-sm text-muted-foreground">
        {t(WEBUI.plugins.noConfigFields)}
      </div>
    );
  }

  const renderFields = (items: ConfigField[], framed = true) => (
    <div
      className={cn(
        "w-full",
        framed &&
          "overflow-hidden rounded-lg border border-border/70 bg-background/25",
      )}
    >
      {items.map((field) => {
        const configured = Object.prototype.hasOwnProperty.call(
          configuredValues,
          field.key,
        );
        return (
          <ConfigFieldRow
            key={field.key}
            field={field}
            plugins={plugins}
            value={values[field.key]}
            configuredValue={configuredValues[field.key]}
            configured={configured}
            onChange={(value) => updateConfig(field.key, value)}
            onReset={() => resetConfig(field.key)}
            defaultArrayObjectCollapsed={defaultArrayObjectCollapsed}
            readOnly={readOnly}
          />
        );
      })}
    </div>
  );

  return (
    <FieldGroup>
      {regularFields.length > 0 && renderFields(regularFields)}
      {advancedFields.length > 0 && (
        <AdvancedSettingsSection
          defaultOpen={hasConfiguredAdvancedValue}
          contentClassName="p-0"
        >
          {renderFields(advancedFields, false)}
        </AdvancedSettingsSection>
      )}
    </FieldGroup>
  );
}

export function hasConfiguredAdvancedFields(
  fields: ConfigField[],
  configuredValues: Record<string, unknown>,
) {
  return fields.some(
    (field) =>
      field.advanced &&
      Object.prototype.hasOwnProperty.call(configuredValues, field.key),
  );
}

function isStructuralConfigField(field: ConfigField): boolean {
  return ["array", "object", "record"].includes(field.type);
}

function ConfigFieldRow({
  field,
  plugins,
  value,
  configuredValue,
  configured,
  onChange,
  onReset,
  defaultArrayObjectCollapsed,
  readOnly,
}: {
  field: ConfigField;
  plugins: PluginInstance[];
  value: unknown;
  configuredValue?: unknown;
  configured: boolean;
  onChange: (value: unknown) => void;
  onReset: () => void;
  defaultArrayObjectCollapsed: boolean;
  readOnly: boolean;
}) {
  return (
    <Field className="grid min-w-0 gap-2.5 border-b border-border/60 px-3 py-2.5 last:border-b-0 @md/field-group:grid-cols-[minmax(9rem,0.8fr)_minmax(0,1.4fr)] @md/field-group:gap-5">
      <div className="min-w-0 space-y-1">
        <ConfigFieldLabel field={field} />
        {field.description && (
          <p className="text-xs leading-5 font-normal text-muted-foreground">
            {field.description}
          </p>
        )}
      </div>
      <div className="min-w-0 space-y-1.5">
        <div className="flex min-w-0 items-start gap-1.5">
          <div className="min-w-0 flex-1">
            <ConfigFieldControl
              field={field}
              plugins={plugins}
              value={value}
              configuredValue={configuredValue}
              configured={configured}
              onChange={onChange}
              defaultArrayObjectCollapsed={defaultArrayObjectCollapsed}
              readOnly={readOnly}
            />
          </div>
          <ConfigFieldResetButton
            field={field}
            configured={configured}
            onReset={onReset}
            readOnly={readOnly}
          />
        </div>
        {!readOnly && <ConfigFieldExample field={field} />}
      </div>
    </Field>
  );
}

function ConfigFieldLabel({ field }: { field: ConfigField }) {
  const { t } = useI18n();
  const docs = field.docs;

  return (
    <FieldLabel className="flex items-center gap-1.5 font-normal">
      <span>{field.label}</span>
      {field.required && <span className="text-destructive">*</span>}
      {docs && (
        <Popover>
          <PopoverTrigger asChild>
            <button
              type="button"
              className="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              aria-label={t(WEBUI.plugins.configHelpLabel, {
                label: field.label,
              })}
            >
              <Info className="h-3.5 w-3.5" />
            </button>
          </PopoverTrigger>
          <PopoverContent
            side="top"
            align="start"
            className="max-h-[min(30rem,70vh)] w-[min(28rem,calc(100vw-2rem))] overflow-y-auto p-3"
          >
            <FieldDocsContent docs={docs} />
          </PopoverContent>
        </Popover>
      )}
    </FieldLabel>
  );
}

function FieldDocsContent({ docs }: { docs: string }) {
  const { t } = useI18n();
  const sections = parseFieldDocs(docs, t(WEBUI.plugins.docsDefaultGroup));

  return (
    <div className="space-y-3 text-xs leading-relaxed text-popover-foreground">
      {sections.spec.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {sections.spec.map((item) => (
            <span
              key={item}
              className="rounded-md border bg-muted/40 px-1.5 py-0.5 font-mono text-[0.7rem] text-muted-foreground"
            >
              {renderInlineCode(item)}
            </span>
          ))}
        </div>
      )}

      {sections.summary.length > 0 && (
        <div className="space-y-1.5">
          {sections.summary.map((line) => (
            <p key={line}>{renderInlineCode(line)}</p>
          ))}
        </div>
      )}

      {sections.groups.map((group) => (
        <div key={group.title} className="space-y-1.5">
          <div className="text-[0.7rem] font-medium text-muted-foreground">
            {group.title}
          </div>
          <div className="space-y-1">
            {group.items.map((item, index) => (
              <FieldDocsBullet
                key={`${group.title}-${index}-${item.text}`}
                item={item}
              />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

interface FieldDocsBulletItem {
  text: string;
  depth: number;
}

interface FieldDocsGroup {
  title: string;
  items: FieldDocsBulletItem[];
}

function FieldDocsBullet({ item }: { item: FieldDocsBulletItem }) {
  return (
    <div
      className="grid grid-cols-[0.55rem_1fr] gap-1"
      style={{ paddingLeft: `${Math.min(item.depth, 3) * 0.75}rem` }}
    >
      <span className="pt-[0.42rem]">
        <span className="block h-1 w-1 rounded-full bg-muted-foreground/70" />
      </span>
      <span>{renderInlineCode(item.text)}</span>
    </div>
  );
}

function parseFieldDocs(
  docs: string,
  defaultGroupTitle: string,
): {
  spec: string[];
  summary: string[];
  groups: FieldDocsGroup[];
} {
  const spec: string[] = [];
  const summary: string[] = [];
  const groups: FieldDocsGroup[] = [];
  let currentGroup: FieldDocsGroup | null = null;

  for (const rawLine of docs.split("\n")) {
    const trimmed = rawLine.trim();
    if (!trimmed) continue;

    const bullet = trimmed.startsWith("- ") ? trimmed.slice(2) : trimmed;
    const depth = Math.max(
      0,
      Math.floor((rawLine.length - rawLine.trimStart().length) / 2),
    );
    const labelMatch = bullet.match(/^([^：:]{2,12})[：:](.*)$/);

    if (depth === 0 && labelMatch) {
      const [, label, value] = labelMatch;
      const normalizedValue = value.trim();

      if (label === "类型" || label === "Type") {
        spec.push(
          ...normalizedValue
            .split(/[；;]/)
            .map((item) => item.trim())
            .filter(Boolean),
        );
        currentGroup = null;
        continue;
      }

      if (
        label === "必填" ||
        label === "默认值" ||
        label === "单位" ||
        label === "Required" ||
        label === "Default" ||
        label === "Unit"
      ) {
        if (normalizedValue) spec.push(`${label}：${normalizedValue}`);
        currentGroup = null;
        continue;
      }

      if (label === "作用" || label === "Purpose") {
        if (normalizedValue) summary.push(normalizedValue);
        currentGroup = null;
        continue;
      }

      currentGroup = {
        title: label,
        items: normalizedValue ? [{ text: normalizedValue, depth: 0 }] : [],
      };
      groups.push(currentGroup);
      continue;
    }

    if (!currentGroup) {
      currentGroup = { title: defaultGroupTitle, items: [] };
      groups.push(currentGroup);
    }

    currentGroup.items.push({ text: bullet, depth });
  }

  return { spec, summary, groups };
}

function renderInlineCode(text: string): ReactNode {
  const parts = text.split(/(`[^`]+`)/g);

  return parts.map((part, index) => {
    if (part.startsWith("`") && part.endsWith("`")) {
      return (
        <code
          key={index}
          className="rounded bg-muted px-1 py-0.5 font-mono text-[0.72rem]"
        >
          {part.slice(1, -1)}
        </code>
      );
    }

    return <Fragment key={index}>{part}</Fragment>;
  });
}

interface ConfigFieldControlProps {
  field: ConfigField;
  plugins: PluginInstance[];
  value: unknown;
  configuredValue?: unknown;
  configured?: boolean;
  onChange: (value: unknown) => void;
  defaultArrayObjectCollapsed: boolean;
  readOnly: boolean;
}

function ConfigFieldControl(props: ConfigFieldControlProps) {
  if (props.readOnly && !isStructuralConfigField(props.field)) {
    return (
      <ConfigFieldReadValue
        field={props.field}
        value={props.value}
        configured={props.configured ?? props.configuredValue !== undefined}
      />
    );
  }
  return <ConfigFieldInput {...props} />;
}

function ConfigFieldInput({
  field,
  plugins,
  value,
  configuredValue,
  configured = configuredValue !== undefined,
  onChange,
  defaultArrayObjectCollapsed,
  readOnly,
}: ConfigFieldControlProps) {
  const { t } = useI18n();
  const configModel = useAppStore((s) => s.configModel);
  const example =
    getConfigFieldExample(field) ??
    (field.type === "duration" ? "3s" : undefined);
  const examplePlaceholder = example
    ? t(WEBUI.plugins.examplePlaceholder, { value: example })
    : undefined;
  const displayValue = resolveConfigFieldDisplayValue(value, field.default);
  const inherited = !configured && field.default !== undefined;
  const inheritedControlClassName = inherited
    ? "border-border/70 bg-muted/20 text-muted-foreground"
    : undefined;
  const examplePlaceholderClassName = example
    ? "placeholder:text-config-example/80"
    : undefined;

  switch (field.type) {
    case "text":
      return (
        <Input
          value={(displayValue as string) || ""}
          onChange={(e) => onChange(e.target.value)}
          placeholder={examplePlaceholder}
          className={cn(
            "font-mono text-sm",
            inheritedControlClassName,
            examplePlaceholderClassName,
          )}
          disabled={readOnly}
        />
      );
    case "password":
      return (
        <Input
          type="password"
          value={(displayValue as string) || ""}
          onChange={(e) => onChange(e.target.value)}
          placeholder={examplePlaceholder}
          className={cn(
            "font-mono text-sm",
            inheritedControlClassName,
            examplePlaceholderClassName,
          )}
          disabled={readOnly}
          autoComplete="new-password"
        />
      );
    case "time":
      return (
        <Input
          type="time"
          step={60}
          value={typeof displayValue === "string" ? displayValue : ""}
          onChange={(event) => onChange(event.target.value)}
          placeholder={examplePlaceholder}
          className={cn(
            "font-mono text-sm",
            inheritedControlClassName,
            examplePlaceholderClassName,
          )}
          disabled={readOnly}
        />
      );
    case "number":
      return (
        <Input
          type="number"
          value={(displayValue as number) ?? ""}
          onChange={(e) =>
            onChange(e.target.value ? Number(e.target.value) : "")
          }
          placeholder={examplePlaceholder}
          className={cn(
            "font-mono text-sm",
            inheritedControlClassName,
            examplePlaceholderClassName,
          )}
          disabled={readOnly}
        />
      );
    case "textarea":
      return (
        <Textarea
          value={(displayValue as string) || ""}
          onChange={(e) => onChange(e.target.value)}
          placeholder={examplePlaceholder}
          className={cn(
            "min-h-[80px] font-mono text-sm",
            inheritedControlClassName,
            examplePlaceholderClassName,
          )}
          disabled={readOnly}
        />
      );
    case "array":
      if (field.arrayPresentation) {
        return (
          <ArrayChoiceFieldEditor
            field={field}
            value={Array.isArray(value) ? value : []}
            onChange={onChange}
            readOnly={readOnly}
          />
        );
      }
      if (field.item || field.itemOptions) {
        return (
          <SchemaArrayFieldEditor
            field={field}
            plugins={plugins}
            value={Array.isArray(value) ? value : []}
            configuredValue={configuredValue}
            onChange={onChange}
            defaultArrayObjectCollapsed={defaultArrayObjectCollapsed}
            readOnly={readOnly}
          />
        );
      }

      return (
        <ArrayFieldEditor
          field={field}
          plugins={plugins}
          value={normalizeArrayValue(value)}
          configuredValue={configuredValue}
          onChange={onChange}
          readOnly={readOnly}
        />
      );
    case "duration":
      return (
        <Input
          value={(displayValue as string) || ""}
          onChange={(e) => onChange(e.target.value)}
          placeholder={examplePlaceholder}
          className={cn(
            "font-mono text-sm",
            inheritedControlClassName,
            examplePlaceholderClassName,
          )}
          disabled={readOnly}
        />
      );
    case "json":
      return (
        <Textarea
          value={
            typeof displayValue === "string"
              ? displayValue
              : displayValue === undefined
                ? ""
                : JSON.stringify(displayValue, null, 2)
          }
          onChange={(e) => onChange(e.target.value)}
          placeholder={examplePlaceholder}
          className={cn(
            "min-h-[120px] font-mono text-sm",
            inheritedControlClassName,
            examplePlaceholderClassName,
          )}
          disabled={readOnly}
        />
      );
    case "object":
      if (!field.fields) return null;
      const objectValue =
        value && typeof value === "object" && !Array.isArray(value)
          ? (value as Record<string, unknown>)
          : createDefaultPluginConfigValues(field.fields);
      if (field.preserveEmptyObject) {
        const present = isPresentOptionalObject(objectValue);
        if (readOnly && !present) {
          return (
            <span className="block py-1 text-sm text-muted-foreground">
              {t(WEBUI.common.unconfigured)}
            </span>
          );
        }
        return (
          <div className="space-y-4">
            {!readOnly && (
              <div className="flex items-center gap-2">
                <Switch
                  checked={present}
                  onCheckedChange={(checked) =>
                    onChange({
                      ...objectValue,
                      [OBJECT_PRESENCE_KEY]: checked,
                    })
                  }
                  aria-label={`${field.label}: ${t(WEBUI.common.enabled)}`}
                />
                <span className="text-sm text-muted-foreground">
                  {t(WEBUI.common.enabled)}
                </span>
              </div>
            )}
            {present && (
              <ObjectFieldEditor
                fields={field.fields}
                plugins={plugins}
                value={objectValue}
                configuredValue={configuredValue}
                onChange={onChange}
                defaultArrayObjectCollapsed={defaultArrayObjectCollapsed}
                readOnly={readOnly}
              />
            )}
          </div>
        );
      }
      return (
        <ObjectFieldEditor
          fields={field.fields}
          plugins={plugins}
          value={objectValue}
          configuredValue={configuredValue}
          onChange={onChange}
          defaultArrayObjectCollapsed={defaultArrayObjectCollapsed}
          readOnly={readOnly}
        />
      );
    case "record":
      return (
        <RecordFieldEditor
          field={field}
          value={Array.isArray(value) ? (value as RecordItemValue[]) : []}
          onChange={onChange}
          readOnly={readOnly}
        />
      );
    case "select":
      const selectDisplayValue = displayValue;
      const selectValue =
        selectDisplayValue == null || selectDisplayValue === ""
          ? OPTIONAL_SELECT_VALUE
          : String(selectDisplayValue);
      const options = withCurrentSelectOption(
        resolveSelectOptions(field, configModel),
        selectValue,
      );
      return (
        <Select
          value={selectValue}
          onValueChange={(next) => {
            if (next === OPTIONAL_SELECT_VALUE) {
              onChange("");
              return;
            }
            const opt = options.find((o) => String(o.value) === next);
            onChange(opt ? opt.value : next);
          }}
          disabled={readOnly}
        >
          <SelectTrigger className={cn("w-full", inheritedControlClassName)}>
            <SelectValue placeholder={t(WEBUI.plugins.selectPlaceholder)} />
          </SelectTrigger>
          <SelectContent>
            {field.dynamicOptions && !field.required && (
              <SelectItem value={OPTIONAL_SELECT_VALUE}>
                {t(WEBUI.common.unconfigured)}
              </SelectItem>
            )}
            {options.map((opt) => (
              <SelectItem key={String(opt.value)} value={String(opt.value)}>
                {opt.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      );
    case "switch":
      return (
        <Switch
          checked={Boolean(displayValue)}
          onCheckedChange={onChange}
          disabled={readOnly}
          className={cn(inherited && "opacity-75")}
        />
      );
    case "reference":
      const referenceValue = stripInvertPrefix(value);
      const referenceInverted =
        typeof value === "string" && value.startsWith("!");
      const referenceCanInvert =
        field.allowInvert || field.referenceTypes?.includes("matcher") || false;

      return (
        <div className="flex items-center gap-2">
          {referenceCanInvert && (
            <InvertCheckbox
              checked={referenceInverted}
              disabled={readOnly || !referenceValue}
              onCheckedChange={(checked) =>
                onChange(
                  `${checked ? "!" : ""}${field.referencePrefix ?? ""}${stripReferencePrefix(referenceValue)}`,
                )
              }
            />
          )}
          <PluginReferencePicker
            plugins={plugins}
            value={referenceValue}
            referenceTypes={field.referenceTypes}
            referencePlugins={field.referencePlugins}
            disabled={readOnly}
            allowCreate
            onChange={(nextValue) =>
              onChange(
                `${referenceInverted ? "!" : ""}${field.referencePrefix ?? ""}${nextValue}`,
              )
            }
          />
          {!field.required && referenceValue && (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant="outline"
                  size="icon-lg"
                  disabled={readOnly}
                  aria-label={t(WEBUI.plugins.clearSelection)}
                  onClick={() => onChange("")}
                >
                  <X />
                </Button>
              </TooltipTrigger>
              <TooltipContent sideOffset={6}>
                {t(WEBUI.plugins.clearSelection)}
              </TooltipContent>
            </Tooltip>
          )}
        </div>
      );
    default:
      return null;
  }
}

function ConfigArrayEmptyState({
  field,
  inherited,
}: {
  field: ConfigField;
  inherited: boolean;
}) {
  const { t } = useI18n();
  const example = getConfigFieldExample(field);
  const defaultItems =
    inherited && Array.isArray(field.default) ? field.default : [];
  const primitiveDefaultItems = defaultItems.filter(
    (item): item is string | number | boolean =>
      ["string", "number", "boolean"].includes(typeof item),
  );

  return (
    <div className="rounded-lg border border-dashed border-border/70 bg-muted/10 px-3 py-3 text-sm text-muted-foreground">
      {primitiveDefaultItems.length === defaultItems.length &&
      primitiveDefaultItems.length > 0 ? (
        <div className="flex flex-wrap gap-x-3 gap-y-1.5">
          {primitiveDefaultItems.map((item, index) => (
            <code
              key={`${String(item)}-${index}`}
              className="font-mono text-xs text-muted-foreground"
            >
              {String(item)}
            </code>
          ))}
        </div>
      ) : (
        <p>
          {defaultItems.length > 0
            ? t(WEBUI.common.itemCount, { count: defaultItems.length })
            : t(WEBUI.plugins.emptyConfigItems)}
        </p>
      )}
      {example && (
        <div className="mt-2 flex min-w-0 items-start gap-2 text-xs">
          <span className="text-config-example/80 shrink-0">
            {t(WEBUI.plugins.configExampleLabel)}
          </span>
          <code className="text-config-example min-w-0 whitespace-pre-wrap break-all font-mono">
            {example}
          </code>
        </div>
      )}
    </div>
  );
}

function ConfigReadCollection({
  values,
  inherited = false,
}: {
  values: unknown[];
  inherited?: boolean;
}) {
  const { t } = useI18n();
  if (values.length === 0) {
    return (
      <span className="block py-1 text-sm text-muted-foreground">
        {t(WEBUI.common.unconfigured)}
      </span>
    );
  }

  return (
    <div className="flex min-w-0 flex-wrap gap-x-3 gap-y-1.5 py-1">
      {values.map((value, index) => (
        <code
          key={`${formatConfigFieldDefaultValue(value)}-${index}`}
          className={cn(
            "min-w-0 whitespace-pre-wrap break-all font-mono text-sm leading-5",
            inherited ? "text-muted-foreground" : "text-foreground",
          )}
        >
          {formatConfigFieldDefaultValue(value)}
        </code>
      ))}
    </div>
  );
}

function ArrayFieldEditor({
  field,
  plugins,
  value,
  configuredValue,
  onChange,
  readOnly,
}: {
  field: ConfigField;
  plugins: PluginInstance[];
  value: ArrayItemValue[];
  configuredValue?: unknown;
  onChange: (items: ArrayItemValue[]) => void;
  readOnly: boolean;
}) {
  const { t } = useI18n();
  if (readOnly) {
    const inheritedItems =
      configuredValue === undefined && Array.isArray(field.default)
        ? field.default
        : undefined;
    const displayItems =
      inheritedItems ??
      value.map((item) => serializeArrayItem(item)).filter(Boolean);
    return (
      <ConfigReadCollection
        values={displayItems}
        inherited={inheritedItems !== undefined}
      />
    );
  }

  const addItem = () => {
    onChange([
      ...value,
      {
        id: createArrayItemId(),
        syntax: inferDefaultSyntax(field),
        value: "",
        referenceTypes: inferReferenceTypes(field),
      },
    ]);
  };

  const updateItem = (id: string, patch: Partial<ArrayItemValue>) => {
    onChange(
      value.map((item) =>
        item.id === id
          ? {
              ...item,
              ...patch,
              value:
                patch.syntax && patch.syntax !== item.syntax
                  ? ""
                  : (patch.value ?? item.value),
            }
          : item,
      ),
    );
  };

  return (
    <div className="space-y-2">
      {value.length > 0 ? (
        value.map((item) => (
          <div
            key={item.id}
            className="grid gap-2 rounded-lg border border-border bg-background/60 p-2 sm:grid-cols-[8.5rem_1fr_auto]"
          >
            <Select
              value={item.syntax}
              onValueChange={(syntax) =>
                updateItem(item.id, { syntax: syntax as ArrayItemSyntax })
              }
              disabled={readOnly}
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {getSyntaxOptions(field).map((syntax) => (
                  <SelectItem key={syntax} value={syntax}>
                    {t(ARRAY_SYNTAX_KEYS[syntax])}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <ArrayItemInput
              item={item}
              field={field}
              plugins={plugins}
              onChange={(patch) => updateItem(item.id, patch)}
              readOnly={readOnly}
            />
            {!readOnly && (
              <Button
                type="button"
                variant="outline"
                size="icon"
                className="sm:self-start"
                onClick={() =>
                  onChange(value.filter((entry) => entry.id !== item.id))
                }
              >
                <Minus className="h-4 w-4" />
              </Button>
            )}
          </div>
        ))
      ) : (
        <ConfigArrayEmptyState
          field={field}
          inherited={
            field.default !== undefined && configuredValue === undefined
          }
        />
      )}

      {!readOnly && (
        <Button type="button" variant="outline" size="sm" onClick={addItem}>
          <Plus className="mr-1.5 h-4 w-4" />
          {t(WEBUI.plugins.addConfigItem)}
        </Button>
      )}
    </div>
  );
}

function ObjectFieldEditor({
  fields,
  plugins,
  value,
  configuredValue,
  onChange,
  defaultArrayObjectCollapsed,
  readOnly,
}: {
  fields: ConfigField[];
  plugins: PluginInstance[];
  value: Record<string, unknown>;
  configuredValue?: unknown;
  onChange: (value: Record<string, unknown>) => void;
  defaultArrayObjectCollapsed: boolean;
  readOnly: boolean;
}) {
  const configuredValues =
    configuredValue &&
    typeof configuredValue === "object" &&
    !Array.isArray(configuredValue)
      ? (configuredValue as Record<string, unknown>)
      : {};
  const regularFields = fields.filter((field) => !field.advanced);
  const advancedFields = fields.filter((field) => field.advanced);

  const renderFields = (items: ConfigField[]) =>
    items.map((field) => {
      if (field.timeRange?.role === "end") return null;
      if (field.timeRange?.role === "start") {
        const endField = findTimeRangePair(fields, field);
        if (endField) {
          return (
            <TimeRangeFieldEditor
              key={field.timeRange.id}
              startField={field}
              endField={endField}
              value={value}
              configured={
                Object.prototype.hasOwnProperty.call(
                  configuredValues,
                  field.key,
                ) ||
                Object.prototype.hasOwnProperty.call(
                  configuredValues,
                  endField.key,
                )
              }
              onChange={onChange}
              readOnly={readOnly}
            />
          );
        }
      }

      const configured = Object.prototype.hasOwnProperty.call(
        configuredValues,
        field.key,
      );
      return (
        <ConfigFieldRow
          key={field.key}
          field={field}
          plugins={plugins}
          value={value[field.key]}
          configuredValue={configuredValues[field.key]}
          configured={configured}
          onChange={(nextFieldValue) =>
            onChange({ ...value, [field.key]: nextFieldValue })
          }
          onReset={() => {
            onChange(omitConfigFieldValues(value, [field.key]));
          }}
          defaultArrayObjectCollapsed={defaultArrayObjectCollapsed}
          readOnly={readOnly}
        />
      );
    });

  return (
    <FieldGroup className="gap-3">
      {regularFields.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-border/60 bg-muted/10">
          {renderFields(regularFields)}
        </div>
      )}
      {advancedFields.length > 0 && (
        <AdvancedSettingsSection
          defaultOpen={hasConfiguredAdvancedFields(
            advancedFields,
            configuredValues,
          )}
          contentClassName="p-0"
        >
          <div>{renderFields(advancedFields)}</div>
        </AdvancedSettingsSection>
      )}
    </FieldGroup>
  );
}

function TimeRangeFieldEditor({
  startField,
  endField,
  value,
  configured,
  onChange,
  readOnly,
}: {
  startField: ConfigField;
  endField: ConfigField;
  value: Record<string, unknown>;
  configured: boolean;
  onChange: (value: Record<string, unknown>) => void;
  readOnly: boolean;
}) {
  const { locale, t } = useI18n();
  const rawStart = value[startField.key];
  const rawEnd = value[endField.key];
  const defaultStart = startField.timeRange?.defaultValue ?? "09:00";
  const defaultEnd = endField.timeRange?.defaultValue ?? "18:00";
  const startExample = getConfigFieldExample(startField) ?? defaultStart;
  const endExample = getConfigFieldExample(endField) ?? defaultEnd;
  const rawStartValue = typeof rawStart === "string" ? rawStart : "";
  const rawEndValue = typeof rawEnd === "string" ? rawEnd : "";
  const hasStart = Boolean(rawStartValue);
  const hasEnd = Boolean(rawEndValue);
  const isUnrestricted = !hasStart && !hasEnd;
  const start = rawStartValue || defaultStart;
  const end = rawEndValue || defaultEnd;
  const incomplete = hasStart !== hasEnd;
  const equal = hasStart && hasEnd && start === end;
  const invalid =
    (hasStart && !isValidTimeValue(start)) ||
    (hasEnd && !isValidTimeValue(end));
  const error = incomplete
    ? t(WEBUI.plugins.timeRangeIncomplete)
    : equal
      ? t(WEBUI.plugins.timeRangeEqual)
      : invalid
        ? t(WEBUI.plugins.timeRangeInvalid)
        : null;

  const updateRange = (startValue: string, endValue: string) => {
    onChange({
      ...value,
      [startField.key]: startValue,
      [endField.key]: endValue,
    });
  };

  const toPickerValue = (time: string) => {
    const [hour, minute] = time.split(":").map(Number);
    return dayjs().hour(hour).minute(minute).second(0).millisecond(0);
  };

  const resetRange = () => {
    onChange(omitConfigFieldValues(value, [startField.key, endField.key]));
  };

  return (
    <Field className="grid min-w-0 gap-2.5 border-b border-border/60 px-3 py-2.5 last:border-b-0 @md/field-group:grid-cols-[minmax(9rem,0.8fr)_minmax(0,1.4fr)] @md/field-group:gap-5">
      <div className="min-w-0 space-y-1">
        <FieldLabel className="font-normal">
          {t(WEBUI.plugins.timeRange)}
        </FieldLabel>
        {startField.description && (
          <p className="text-xs leading-5 font-normal text-muted-foreground">
            {startField.description}
          </p>
        )}
      </div>
      <div className="min-w-0 space-y-1.5">
        {readOnly ? (
          <div className="flex min-w-0 flex-wrap items-center gap-2 py-1">
            {isUnrestricted ? (
              <span className="text-sm text-muted-foreground">
                {t(WEBUI.plugins.unrestrictedTime)}
              </span>
            ) : (
              <>
                <code className="font-mono text-sm text-foreground">
                  {start} – {end}
                </code>
                {start > end && (
                  <span className="text-xs text-muted-foreground">
                    {t(WEBUI.plugins.nextDay)}
                  </span>
                )}
              </>
            )}
          </div>
        ) : (
          <>
            <div className="flex min-w-0 flex-wrap items-start justify-between gap-2">
              {isUnrestricted ? (
                <div className="flex items-center gap-2">
                  <span className="rounded-md border border-dashed px-2.5 py-1.5 text-sm text-muted-foreground">
                    {t(WEBUI.plugins.unrestrictedTime)}
                  </span>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => updateRange(defaultStart, defaultEnd)}
                  >
                    {t(WEBUI.plugins.setTimeRange)}
                  </Button>
                </div>
              ) : (
                <div className="flex min-w-0 flex-wrap items-center gap-2">
                  <ConfigProvider
                    locale={locale === "zh-CN" ? zhCN : enUS}
                    theme={{
                      token: {
                        borderRadius: 8,
                        controlHeight: 32,
                        fontFamily: "var(--font-mono)",
                        fontSize: 14,
                      },
                    }}
                  >
                    <TimePicker.RangePicker
                      value={[toPickerValue(start), toPickerValue(end)]}
                      onChange={(_, timeStrings) => {
                        const [nextStart, nextEnd] = timeStrings;
                        if (!nextStart && !nextEnd) {
                          resetRange();
                          return;
                        }
                        if (!nextStart || !nextEnd) return;
                        updateRange(nextStart.slice(0, 5), nextEnd.slice(0, 5));
                      }}
                      className="oxidns-time-range-picker"
                      popupClassName="oxidns-time-range-picker-popup"
                      allowClear
                      format="HH:mm"
                      inputReadOnly
                      minuteStep={1}
                      needConfirm={false}
                      order={false}
                      placeholder={[startField.label, endField.label]}
                    />
                  </ConfigProvider>
                  {hasStart && hasEnd && start > end && (
                    <span className="rounded bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                      {t(WEBUI.plugins.nextDay)}
                    </span>
                  )}
                </div>
              )}
              <ConfigFieldResetButton
                field={startField}
                configured={configured}
                onReset={resetRange}
                readOnly={readOnly}
              />
            </div>
            <FieldDescription className="text-config-example text-xs leading-4">
              {t(WEBUI.plugins.configExampleLabel)}:
              <code className="ml-1 font-mono">
                {startExample} – {endExample}
              </code>
            </FieldDescription>
          </>
        )}
        {error && <p className="text-xs text-destructive">{error}</p>}
      </div>
    </Field>
  );
}

function ArrayChoiceFieldEditor({
  field,
  value,
  onChange,
  readOnly,
}: {
  field: ConfigField;
  value: unknown[];
  onChange: (value: unknown[]) => void;
  readOnly: boolean;
}) {
  const { t } = useI18n();
  const options =
    field.item && "options" in field.item ? (field.item.options ?? []) : [];
  const isWeekdayPicker = field.arrayPresentation === "weekday-chips";
  const isMonthdayPicker = field.arrayPresentation === "calendar-grid";
  const [isMonthdayGridOpen, setIsMonthdayGridOpen] = useState(
    value.length > 0,
  );
  const selected = new Set(
    isWeekdayPicker && value.length === 0
      ? options.map((option) => String(option.value))
      : value.map((entry) => String(entry)),
  );
  const dragSelectionRef = useRef<{
    pointerId: number;
    checked: boolean;
    selected: Set<string>;
  } | null>(null);
  const suppressClickRef = useRef(false);

  const commitSelection = (nextSelected: Set<string>) => {
    const nextValue = options
      .filter((option) => nextSelected.has(String(option.value)))
      .map((option) => option.value);
    onChange(
      isWeekdayPicker && nextValue.length === options.length ? [] : nextValue,
    );
  };

  const updateSelection = (nextSelected: Set<string>) => {
    if (isWeekdayPicker && nextSelected.size === 0) return false;
    commitSelection(nextSelected);
    return true;
  };

  const toggle = (optionValue: string | number, checked: boolean) => {
    const nextSelected = new Set(selected);
    const key = String(optionValue);
    if (checked) nextSelected.add(key);
    else nextSelected.delete(key);
    updateSelection(nextSelected);
  };

  const beginPointerSelection = (
    event: ReactPointerEvent<HTMLButtonElement>,
    optionValue: string | number,
  ) => {
    if (readOnly || event.pointerType !== "mouse" || event.button !== 0) return;
    event.preventDefault();
    const nextSelected = new Set(selected);
    const key = String(optionValue);
    const checked = !nextSelected.has(key);
    if (checked) nextSelected.add(key);
    else nextSelected.delete(key);
    if (!updateSelection(nextSelected)) return;
    dragSelectionRef.current = {
      pointerId: event.pointerId,
      checked,
      selected: nextSelected,
    };
    suppressClickRef.current = true;
  };

  const extendPointerSelection = (
    event: ReactPointerEvent<HTMLButtonElement>,
    optionValue: string | number,
  ) => {
    const session = dragSelectionRef.current;
    if (!session || session.pointerId !== event.pointerId) return;
    const key = String(optionValue);
    if (session.selected.has(key) === session.checked) return;
    const nextSelected = new Set(session.selected);
    if (session.checked) nextSelected.add(key);
    else nextSelected.delete(key);
    if (updateSelection(nextSelected)) session.selected = nextSelected;
  };

  const handleChoiceClick = (
    event: ReactMouseEvent<HTMLButtonElement>,
    optionValue: string | number,
    checked: boolean,
  ) => {
    if (suppressClickRef.current) {
      event.preventDefault();
      suppressClickRef.current = false;
      return;
    }
    toggle(optionValue, !checked);
  };

  useEffect(() => {
    const finishPointerSelection = () => {
      dragSelectionRef.current = null;
      window.setTimeout(() => {
        suppressClickRef.current = false;
      }, 0);
    };
    window.addEventListener("pointerup", finishPointerSelection);
    window.addEventListener("pointercancel", finishPointerSelection);
    return () => {
      window.removeEventListener("pointerup", finishPointerSelection);
      window.removeEventListener("pointercancel", finishPointerSelection);
    };
  }, []);

  const applyPreset = (values: string[]) => {
    commitSelection(new Set(values));
  };

  const isSelectedExactly = (values: string[]) =>
    selected.size === values.length &&
    values.every((value) => selected.has(value));

  const weekdayValues = options.map((option) => String(option.value));
  const workdayValues = weekdayValues.slice(0, 5);
  const weekendValues = weekdayValues.slice(5);
  const showChoices = !isMonthdayPicker || isMonthdayGridOpen;
  const status = isWeekdayPicker
    ? value.length === 0
      ? t(WEBUI.plugins.everyDay)
      : t(WEBUI.plugins.selectedWeekdays, { count: value.length })
    : isMonthdayPicker
      ? value.length === 0
        ? t(WEBUI.plugins.unrestrictedDates)
        : t(WEBUI.plugins.selectedMonthdays, { count: value.length })
      : value.length === 0
        ? t(WEBUI.plugins.unrestricted)
        : t(WEBUI.plugins.selectedCount, { count: value.length });

  if (readOnly) {
    const selectedLabels = options
      .filter((option) => selected.has(String(option.value)))
      .map((option) => option.label);
    return selectedLabels.length > 0 && value.length > 0 ? (
      <ConfigReadCollection values={selectedLabels} />
    ) : (
      <span className="block py-1 text-sm text-muted-foreground">{status}</span>
    );
  }

  return (
    <div className="space-y-2.5 rounded-lg border border-border/70 bg-background/40 p-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="text-xs text-muted-foreground">{status}</span>
        {!readOnly && isWeekdayPicker && (
          <div className="flex flex-wrap items-center gap-1">
            <Button
              type="button"
              variant={isSelectedExactly(weekdayValues) ? "secondary" : "ghost"}
              size="xs"
              onClick={() => applyPreset(weekdayValues)}
            >
              {t(WEBUI.plugins.everyDay)}
            </Button>
            <Button
              type="button"
              variant={isSelectedExactly(workdayValues) ? "secondary" : "ghost"}
              size="xs"
              onClick={() => applyPreset(workdayValues)}
            >
              {t(WEBUI.plugins.workdays)}
            </Button>
            <Button
              type="button"
              variant={isSelectedExactly(weekendValues) ? "secondary" : "ghost"}
              size="xs"
              onClick={() => applyPreset(weekendValues)}
            >
              {t(WEBUI.plugins.weekends)}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="xs"
              onClick={() => onChange([])}
              disabled={value.length === 0}
            >
              {t(WEBUI.plugins.clearSelection)}
            </Button>
          </div>
        )}
        {!readOnly && isMonthdayPicker && (
          <Button
            type="button"
            variant={isMonthdayGridOpen ? "ghost" : "secondary"}
            size="xs"
            onClick={() => {
              if (isMonthdayGridOpen) {
                onChange([]);
                setIsMonthdayGridOpen(false);
              } else {
                setIsMonthdayGridOpen(true);
              }
            }}
          >
            {isMonthdayGridOpen
              ? value.length > 0
                ? t(WEBUI.plugins.clearSelection)
                : t(WEBUI.plugins.unrestrictedDates)
              : t(WEBUI.plugins.specifiedDates)}
          </Button>
        )}
        {!readOnly && !isWeekdayPicker && !isMonthdayPicker && (
          <div className="flex items-center gap-1">
            <Button
              type="button"
              variant="ghost"
              size="xs"
              onClick={() => onChange(options.map((option) => option.value))}
              disabled={value.length === options.length}
            >
              {t(WEBUI.plugins.selectAll)}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="xs"
              onClick={() => onChange([])}
              disabled={value.length === 0}
            >
              {t(WEBUI.plugins.clearSelection)}
            </Button>
          </div>
        )}
      </div>

      {showChoices && (
        <div
          className={cn(
            isMonthdayPicker || isWeekdayPicker
              ? "grid w-full grid-cols-7 gap-1 sm:gap-1.5"
              : "grid gap-1.5 [grid-template-columns:repeat(auto-fit,minmax(6.5rem,1fr))]",
          )}
        >
          {options.map((option) => {
            const optionKey = String(option.value);
            const checked = selected.has(optionKey);
            if (isMonthdayPicker) {
              return (
                <Button
                  key={optionKey}
                  type="button"
                  variant="outline"
                  size="sm"
                  className={cn(
                    "h-8 w-full select-none px-1 font-mono text-xs touch-pan-y",
                    checked &&
                      "border-primary/60 bg-primary/12 font-semibold text-primary hover:bg-primary/18 hover:text-primary",
                  )}
                  aria-pressed={checked}
                  disabled={readOnly}
                  onPointerDown={(event) =>
                    beginPointerSelection(event, option.value)
                  }
                  onPointerEnter={(event) =>
                    extendPointerSelection(event, option.value)
                  }
                  onClick={(event) =>
                    handleChoiceClick(event, option.value, checked)
                  }
                >
                  {option.label}
                </Button>
              );
            }

            if (isWeekdayPicker) {
              return (
                <Button
                  key={optionKey}
                  type="button"
                  variant="outline"
                  size="sm"
                  className={cn(
                    "w-full select-none px-1 touch-pan-y",
                    checked &&
                      "border-primary/60 bg-primary/12 font-semibold text-primary hover:bg-primary/18 hover:text-primary",
                  )}
                  aria-pressed={checked}
                  disabled={readOnly}
                  onPointerDown={(event) =>
                    beginPointerSelection(event, option.value)
                  }
                  onPointerEnter={(event) =>
                    extendPointerSelection(event, option.value)
                  }
                  onClick={(event) =>
                    handleChoiceClick(event, option.value, checked)
                  }
                >
                  {option.label}
                </Button>
              );
            }

            return (
              <label
                key={optionKey}
                className={cn(
                  "flex min-h-9 cursor-pointer items-center gap-2 rounded-md border px-2.5 py-1.5 text-sm transition-colors",
                  checked
                    ? "border-primary/60 bg-primary/8 text-foreground"
                    : "border-border bg-background hover:bg-muted/50",
                  readOnly && "cursor-not-allowed opacity-60",
                )}
              >
                <Checkbox
                  checked={checked}
                  onCheckedChange={(next) =>
                    toggle(option.value, next === true)
                  }
                  disabled={readOnly}
                />
                <span>{option.label}</span>
              </label>
            );
          })}
        </div>
      )}
    </div>
  );
}

function RecordFieldEditor({
  field,
  value,
  onChange,
  readOnly,
}: {
  field: ConfigField;
  value: RecordItemValue[];
  onChange: (value: RecordItemValue[]) => void;
  readOnly: boolean;
}) {
  const { t } = useI18n();
  if (readOnly) {
    if (value.length === 0) {
      return <ConfigReadCollection values={[]} />;
    }
    return (
      <dl className="grid min-w-0 gap-x-4 gap-y-1.5 py-1 @sm/field-group:grid-cols-[minmax(0,12rem)_minmax(0,1fr)]">
        {value.map((item) => (
          <Fragment key={item.id}>
            <dt className="truncate font-mono text-sm text-muted-foreground">
              {item.key}
            </dt>
            <dd className="min-w-0 whitespace-pre-wrap break-all font-mono text-sm text-foreground">
              {item.value}
            </dd>
          </Fragment>
        ))}
      </dl>
    );
  }

  const addItem = () => {
    onChange([...value, { id: createArrayItemId(), key: "", value: "" }]);
  };

  const updateItem = (id: string, patch: Partial<RecordItemValue>) => {
    onChange(
      value.map((item) => (item.id === id ? { ...item, ...patch } : item)),
    );
  };

  return (
    <div className="space-y-2">
      {value.length > 0 ? (
        value.map((item) => (
          <div
            key={item.id}
            className="grid gap-2 rounded-lg border border-border bg-background/60 p-2 sm:grid-cols-[minmax(0,12rem)_1fr_auto]"
          >
            <Input
              value={item.key}
              onChange={(event) =>
                updateItem(item.id, { key: event.target.value })
              }
              placeholder={
                field.keyPlaceholder
                  ? t(WEBUI.plugins.examplePlaceholder, {
                      value: field.keyPlaceholder,
                    })
                  : t(WEBUI.common.key)
              }
              className={cn(
                "font-mono text-sm",
                field.keyPlaceholder && "placeholder:text-config-example/80",
              )}
              disabled={readOnly}
            />
            <Input
              value={item.value}
              onChange={(event) =>
                updateItem(item.id, { value: event.target.value })
              }
              placeholder={
                field.valuePlaceholder
                  ? t(WEBUI.plugins.examplePlaceholder, {
                      value: field.valuePlaceholder,
                    })
                  : t(WEBUI.plugins.valueLabel)
              }
              className={cn(
                "font-mono text-sm",
                field.valuePlaceholder && "placeholder:text-config-example/80",
              )}
              disabled={readOnly}
            />
            {!readOnly && (
              <Button
                type="button"
                variant="outline"
                size="icon"
                className="h-9 w-9"
                onClick={() =>
                  onChange(value.filter((entry) => entry.id !== item.id))
                }
              >
                <Minus className="h-4 w-4" />
              </Button>
            )}
          </div>
        ))
      ) : (
        <ConfigArrayEmptyState field={field} inherited={false} />
      )}

      {!readOnly && (
        <Button type="button" variant="outline" size="sm" onClick={addItem}>
          <Plus className="mr-1.5 h-4 w-4" />
          {t(WEBUI.plugins.addConfigItem)}
        </Button>
      )}
    </div>
  );
}

function SchemaArrayFieldEditor({
  field,
  plugins,
  value,
  configuredValue,
  onChange,
  defaultArrayObjectCollapsed,
  readOnly,
}: {
  field: ConfigField;
  plugins: PluginInstance[];
  value: unknown[];
  configuredValue?: unknown;
  onChange: (items: unknown[]) => void;
  defaultArrayObjectCollapsed: boolean;
  readOnly: boolean;
}) {
  const { t } = useI18n();
  const itemOptions = getArrayFieldItemOptions(field);
  const [selectedOptionKey, setSelectedOptionKey] = useState(
    getChildOptionKey(itemOptions[0]),
  );
  const [collapsedItems, setCollapsedItems] = useState<Record<string, boolean>>(
    {},
  );
  const configuredEntries = Array.isArray(configuredValue)
    ? configuredValue
    : [];
  const inheritedDefault =
    configuredValue === undefined && Array.isArray(field.default);
  const displayedValue =
    readOnly && value.length === 0 && inheritedDefault
      ? normalizeArrayFieldValue(field.default, field)
      : value;

  const addItem = () => {
    const selectedOption =
      itemOptions.find(
        (option) => getChildOptionKey(option) === selectedOptionKey,
      ) ?? itemOptions[0];

    if (!selectedOption) return;

    if (field.itemOptions) {
      onChange([
        ...value,
        {
          id: createArrayItemId(),
          optionKey: getChildOptionKey(selectedOption),
          value: createDefaultArrayItemValue(selectedOption),
        } satisfies SchemaArrayOptionValue,
      ]);
      return;
    }

    onChange([...value, createDefaultArrayItemValue(selectedOption)]);
  };

  const updateItem = (index: number, nextValue: unknown) => {
    onChange(
      value.map((entry, entryIndex) =>
        entryIndex === index ? nextValue : entry,
      ),
    );
  };

  const removeItem = (index: number) => {
    onChange(value.filter((_, entryIndex) => entryIndex !== index));
  };

  const singleItem = itemOptions.length === 1 ? itemOptions[0] : undefined;
  if (
    readOnly &&
    !field.itemOptions &&
    singleItem &&
    singleItem.type !== "object" &&
    singleItem.type !== "array"
  ) {
    return (
      <ConfigReadCollection
        values={displayedValue}
        inherited={inheritedDefault}
      />
    );
  }

  return (
    <div className="space-y-2">
      {displayedValue.length > 0 ? (
        <div className="divide-y divide-border/60 overflow-hidden rounded-lg border border-border/70 bg-background/35">
          {displayedValue.map((entry, index) => {
            const entryKey = getArrayEntryKey(entry, index);
            const child = getArrayEntryChild(entry, field);
            const entryValue = getArrayEntryValue(entry, field);
            const canCollapse = child.type === "object";
            const structuralEntry = canCollapse || child.type === "array";
            const showEntryHeader = shouldShowSchemaArrayEntryHeader(
              field,
              child,
            );
            const entryLabel = getArrayEntryLabel(entry, field, index, t);
            const isCollapsed =
              canCollapse &&
              (collapsedItems[entryKey] ?? defaultArrayObjectCollapsed);
            const control = (
              <SchemaArrayItemControl
                item={child}
                plugins={plugins}
                value={entryValue}
                configuredValue={configuredEntries[index]}
                configured={!inheritedDefault}
                example={getConfigFieldExample(field)}
                onChange={(nextValue) =>
                  updateItem(index, setArrayEntryValue(entry, field, nextValue))
                }
                defaultArrayObjectCollapsed={defaultArrayObjectCollapsed}
                readOnly={readOnly}
              />
            );

            if (structuralEntry) {
              return (
                <div key={entryKey} className="min-w-0">
                  <div className="flex min-h-9 items-center gap-2 px-2.5 py-1.5">
                    <div className="min-w-0 flex-1">
                      {canCollapse ? (
                        <button
                          type="button"
                          className="flex w-full min-w-0 items-center gap-2 text-left text-xs font-medium text-muted-foreground hover:text-foreground"
                          onClick={() =>
                            setCollapsedItems((current) => ({
                              ...current,
                              [entryKey]: !isCollapsed,
                            }))
                          }
                        >
                          <ChevronDown
                            className={`h-4 w-4 shrink-0 transition-transform ${
                              isCollapsed ? "-rotate-90" : ""
                            }`}
                          />
                          <span className="truncate">{entryLabel}</span>
                          {isCollapsed && (
                            <span className="min-w-0 flex-1 truncate font-normal text-foreground">
                              {getObjectSummary(child, entryValue, t)}
                            </span>
                          )}
                        </button>
                      ) : (
                        <span className="text-xs font-medium text-muted-foreground">
                          {entryLabel}
                        </span>
                      )}
                    </div>
                    {!readOnly && (
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-sm"
                        className="shrink-0 text-muted-foreground"
                        onClick={() => removeItem(index)}
                      >
                        <Minus className="h-4 w-4" />
                      </Button>
                    )}
                  </div>
                  {!isCollapsed && (
                    <div className="border-t border-border/60 bg-muted/5 p-2">
                      {control}
                    </div>
                  )}
                </div>
              );
            }

            return (
              <div
                key={entryKey}
                className="flex min-w-0 items-start gap-2 px-2.5 py-1.5"
              >
                {showEntryHeader && (
                  <span className="mt-1.5 inline-flex h-6 shrink-0 items-center rounded-md bg-muted px-2 text-[0.7rem] font-medium text-muted-foreground">
                    {entryLabel}
                  </span>
                )}
                <div className="min-w-0 flex-1">{control}</div>
                {!readOnly && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    className="mt-0.5 shrink-0 text-muted-foreground"
                    onClick={() => removeItem(index)}
                  >
                    <Minus className="h-4 w-4" />
                  </Button>
                )}
              </div>
            );
          })}
        </div>
      ) : (
        <ConfigArrayEmptyState
          field={field}
          inherited={
            field.default !== undefined && configuredValue === undefined
          }
        />
      )}

      {!readOnly && (
        <div className="flex flex-wrap items-center gap-2">
          {field.itemOptions && itemOptions.length > 1 && (
            <Select
              value={selectedOptionKey}
              onValueChange={setSelectedOptionKey}
            >
              <SelectTrigger className="h-9 w-36">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {itemOptions.map((option) => (
                  <SelectItem
                    key={getChildOptionKey(option)}
                    value={getChildOptionKey(option)}
                  >
                    {getChildLabel(option, t)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
          <Button type="button" variant="outline" size="sm" onClick={addItem}>
            <Plus className="mr-1.5 h-4 w-4" />
            {t(WEBUI.plugins.addConfigItem)}
          </Button>
        </div>
      )}
    </div>
  );
}

function SchemaArrayItemControl({
  item,
  plugins,
  value,
  configuredValue,
  configured,
  example,
  onChange,
  defaultArrayObjectCollapsed,
  readOnly,
}: {
  item: ConfigFieldChild;
  plugins: PluginInstance[];
  value: unknown;
  configuredValue?: unknown;
  configured?: boolean;
  example?: string;
  onChange: (value: unknown) => void;
  defaultArrayObjectCollapsed: boolean;
  readOnly: boolean;
}) {
  const { t } = useI18n();
  if (item.type === "object") {
    const objectValue =
      value && typeof value === "object" && !Array.isArray(value)
        ? (value as Record<string, unknown>)
        : {};

    return (
      <ObjectFieldEditor
        fields={item.fields}
        plugins={plugins}
        value={objectValue}
        configuredValue={configuredValue}
        onChange={onChange}
        defaultArrayObjectCollapsed={defaultArrayObjectCollapsed}
        readOnly={readOnly}
      />
    );
  }

  if (item.type === "array") {
    return (
      <SchemaArrayFieldEditor
        field={arrayItemToConfigField(
          item,
          example,
          t(WEBUI.plugins.valueLabel),
        )}
        plugins={plugins}
        value={Array.isArray(value) ? value : []}
        configuredValue={configuredValue}
        onChange={onChange}
        defaultArrayObjectCollapsed={defaultArrayObjectCollapsed}
        readOnly={readOnly}
      />
    );
  }

  return (
    <ConfigFieldControl
      field={arrayItemToConfigField(item, example, t(WEBUI.plugins.valueLabel))}
      plugins={plugins}
      value={value}
      configuredValue={configuredValue}
      configured={configured}
      onChange={onChange}
      defaultArrayObjectCollapsed={defaultArrayObjectCollapsed}
      readOnly={readOnly}
    />
  );
}

function ArrayItemInput({
  item,
  field,
  plugins,
  onChange,
  readOnly,
}: {
  item: ArrayItemValue;
  field: ConfigField;
  plugins: PluginInstance[];
  onChange: (patch: Partial<ArrayItemValue>) => void;
  readOnly: boolean;
}) {
  const { t } = useI18n();
  const example = getConfigFieldExample(field)?.split("\n")[0];

  if (item.syntax === "plugin") {
    const referenceTypes = item.referenceTypes ?? inferReferenceTypes(field);
    const canInvert = referenceTypes.includes("matcher");

    return (
      <div className="flex min-w-0 items-center gap-2">
        {canInvert && (
          <InvertCheckbox
            checked={!!item.invert}
            disabled={readOnly || !stripInvertPrefix(item.value)}
            onCheckedChange={(checked) =>
              onChange({
                invert: checked,
                value: checked
                  ? `!$${stripInvertPrefix(item.value)}`
                  : `$${stripInvertPrefix(item.value)}`,
              })
            }
          />
        )}
        <PluginReferencePicker
          plugins={plugins}
          value={stripInvertPrefix(item.value)}
          referenceTypes={referenceTypes}
          referencePlugins={field.referencePlugins}
          disabled={readOnly}
          allowCreate
          onChange={(nextValue) =>
            onChange({
              value: item.invert ? `!$${nextValue}` : `$${nextValue}`,
            })
          }
        />
      </div>
    );
  }

  return (
    <Input
      value={item.value}
      onChange={(event) => onChange({ value: event.target.value })}
      placeholder={
        example
          ? t(WEBUI.plugins.examplePlaceholder, { value: example })
          : t(WEBUI.plugins.valueLabel)
      }
      className={cn(
        "font-mono text-sm",
        example && "placeholder:text-config-example/80",
      )}
      disabled={readOnly}
    />
  );
}

function normalizeArrayValue(value: unknown): ArrayItemValue[] {
  if (!Array.isArray(value)) {
    if (typeof value === "string" && value.trim()) {
      return value
        .split("\n")
        .map((line) => line.trim())
        .filter(Boolean)
        .map(createArrayItemFromString);
    }
    return [];
  }

  return value.map((item) =>
    typeof item === "string"
      ? createArrayItemFromString(item)
      : (item as ArrayItemValue),
  );
}

function normalizeRecordValue(value: unknown): RecordItemValue[] {
  if (!value || typeof value !== "object" || Array.isArray(value)) return [];
  return Object.entries(value as Record<string, unknown>).map(
    ([key, entry]) => ({
      id: createArrayItemId(),
      key,
      value: typeof entry === "string" ? entry : String(entry ?? ""),
    }),
  );
}

function serializeRecordValue(value: RecordItemValue[]) {
  return value.reduce<Record<string, string>>((record, item) => {
    const key = item.key.trim();
    if (!key) return record;
    record[key] = item.value;
    return record;
  }, {});
}

function normalizeArrayFieldValue(
  value: unknown,
  field: ConfigField,
): unknown[] {
  if (field.itemOptions) {
    return normalizeOptionArrayValue(value, field.itemOptions);
  }

  if (field.arrayPresentation && field.item && "options" in field.item) {
    const options = field.item.options ?? [];
    return normalizeArrayInputEntries(value).map((entry) => {
      const normalizedEntry = String(entry).toLowerCase();
      const option = options.find(
        (candidate) =>
          String(candidate.value).toLowerCase() === normalizedEntry ||
          candidate.aliases?.some(
            (alias) => String(alias).toLowerCase() === normalizedEntry,
          ),
      );
      return option?.value ?? entry;
    });
  }

  if (field.item) {
    return normalizeSchemaArrayValue(value, field.item);
  }

  return normalizeArrayValue(value);
}

function serializeArrayFieldValue(value: unknown[], field: ConfigField) {
  if (field.itemOptions) {
    return value
      .map((entry) =>
        serializeOptionArrayEntry(entry as SchemaArrayOptionValue, field),
      )
      .filter((entry) => !isEmptyConfigValue(entry));
  }

  if (field.item) {
    return value
      .map((item) => serializeSchemaArrayItem(item, field.item!))
      .filter((item) => !isEmptyConfigValue(item));
  }

  return value
    .map((item) =>
      typeof item === "string"
        ? item
        : serializeArrayItem(item as ArrayItemValue),
    )
    .filter(Boolean);
}

function normalizeOptionArrayValue(
  value: unknown,
  itemOptions: ConfigFieldChild[],
): SchemaArrayOptionValue[] {
  const entries = normalizeArrayInputEntries(value);
  if (entries.length === 0) return [];

  return entries.map((entry) => {
    const option = inferArrayItemOption(entry, itemOptions);
    return {
      id: createArrayItemId(),
      optionKey: getChildOptionKey(option),
      value: normalizeSchemaValue(entry, option),
    };
  });
}

function normalizeSchemaArrayValue(
  value: unknown,
  item: ConfigFieldChild,
): unknown[] {
  return normalizeArrayInputEntries(value).map((entry) =>
    normalizeSchemaValue(entry, item),
  );
}

function normalizeArrayInputEntries(value: unknown): unknown[] {
  if (Array.isArray(value)) return value;
  if (typeof value === "string" && value.trim()) {
    return value
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean);
  }
  return [];
}

function normalizeSchemaValue(value: unknown, item: ConfigFieldChild): unknown {
  if (item.type === "object") {
    return value && typeof value === "object" && !Array.isArray(value)
      ? createPluginConfigFormValues(
          item.fields,
          value as Record<string, unknown>,
        )
      : createDefaultPluginConfigValues(item.fields);
  }

  if (item.type === "array") {
    const field = arrayItemToConfigField(item);
    return normalizeArrayFieldValue(value, field);
  }

  return value;
}

function serializeSchemaArrayItem(
  value: unknown,
  item: ConfigFieldChild,
): unknown {
  if (item.type === "object") {
    if (!value || typeof value !== "object" || Array.isArray(value)) return {};
    return serializePluginConfigValues(
      item.fields,
      value as Record<string, unknown>,
    );
  }

  if (item.type === "array") {
    return Array.isArray(value)
      ? serializeArrayFieldValue(value, arrayItemToConfigField(item))
      : [];
  }

  if (item.type === "reference") {
    const tag = stripReferencePrefix(value);
    if (!tag) return "";
    const inverted = typeof value === "string" && value.startsWith("!");
    return `${inverted ? "!" : ""}${item.referencePrefix ?? ""}${tag}`;
  }

  return value;
}

function serializeOptionArrayEntry(entry: unknown, field: ConfigField) {
  const options = field.itemOptions ?? [];
  const isFormEntry =
    entry &&
    typeof entry === "object" &&
    !Array.isArray(entry) &&
    "optionKey" in entry &&
    "value" in entry;
  const option = isFormEntry
    ? (options.find(
        (item) =>
          getChildOptionKey(item) ===
          String((entry as SchemaArrayOptionValue).optionKey),
      ) ?? options[0])
    : inferArrayItemOption(entry, options);

  if (!option) return "";
  return serializeSchemaArrayItem(
    isFormEntry ? (entry as SchemaArrayOptionValue).value : entry,
    option,
  );
}

function createDefaultArrayItemValue(item: ConfigFieldChild): unknown {
  if (item.type === "object") {
    return createDefaultPluginConfigValues(item.fields);
  }

  if (item.type === "array") return [];
  if (item.default !== undefined) return item.default;
  if (item.type === "switch") return false;
  return "";
}

function arrayItemToConfigField(
  item: ConfigFieldChild,
  example?: string,
  fallbackLabel?: string,
): ConfigField {
  return {
    key: "value",
    ...item,
    label: item.label ?? fallbackLabel ?? "",
    example: getConfigFieldExample(item) ?? example,
  };
}

function getArrayFieldItemOptions(field: ConfigField): ConfigFieldChild[] {
  if (field.itemOptions?.length) return field.itemOptions;
  if (field.item) return [field.item];
  return [
    {
      optionKey: "input",
      type: "text",
      example: getConfigFieldExample(field)?.split("\n")[0],
    },
  ];
}

function getArrayEntryChild(
  entry: unknown,
  field: ConfigField,
): ConfigFieldChild {
  const options = getArrayFieldItemOptions(field);
  if (!field.itemOptions) return options[0];

  const optionKey =
    entry && typeof entry === "object" && "optionKey" in entry
      ? String((entry as SchemaArrayOptionValue).optionKey)
      : "";
  return (
    options.find((option) => getChildOptionKey(option) === optionKey) ??
    options[0]
  );
}

function getArrayEntryValue(entry: unknown, field: ConfigField) {
  if (!field.itemOptions) return entry;
  return entry && typeof entry === "object" && "value" in entry
    ? (entry as SchemaArrayOptionValue).value
    : "";
}

function resolveSelectOptions(
  field: ConfigField,
  configModel: Record<string, unknown>,
) {
  if (field.dynamicOptions === "outboundProfiles") {
    return getOutboundProfileOptions(configModel);
  }
  return field.options ?? [];
}

function withCurrentSelectOption(
  options: NonNullable<ConfigField["options"]>,
  currentValue: string,
) {
  if (
    currentValue === OPTIONAL_SELECT_VALUE ||
    options.some((option) => String(option.value) === currentValue)
  ) {
    return options;
  }
  return [{ label: currentValue, value: currentValue }, ...options];
}

function getOutboundProfileOptions(configModel: Record<string, unknown>) {
  const network = asRecord(configModel.network);
  const outbound = asRecord(network.outbound);
  const profiles = asRecord(outbound.profiles);
  return Object.keys(profiles)
    .sort((a, b) => a.localeCompare(b))
    .map((name) => ({ label: name, value: name }));
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function getArrayEntryKey(entry: unknown, index: number) {
  if (entry && typeof entry === "object" && "id" in entry) {
    return String((entry as { id: unknown }).id);
  }
  return `item_${index}`;
}

function setArrayEntryValue(
  entry: unknown,
  field: ConfigField,
  value: unknown,
): unknown {
  if (!field.itemOptions) return value;
  const current =
    entry && typeof entry === "object"
      ? (entry as SchemaArrayOptionValue)
      : ({
          id: createArrayItemId(),
          optionKey: getChildOptionKey(getArrayFieldItemOptions(field)[0]),
          value: "",
        } satisfies SchemaArrayOptionValue);
  return { ...current, value };
}

type TFn = (
  key: string,
  params?: Record<string, string | number | boolean | null | undefined>,
) => string;

function getArrayEntryLabel(
  entry: unknown,
  field: ConfigField,
  index: number,
  t: TFn,
) {
  const child = getArrayEntryChild(entry, field);
  return (
    child.label ?? t(WEBUI.plugins.configItemFallback, { index: index + 1 })
  );
}

function getObjectSummary(
  item: ConfigFieldChild,
  value: unknown,
  t: TFn,
): string {
  if (item.type !== "object") return formatSummaryValue(value, t);
  return getObjectSummaryFromFields(item.fields, item.summaryFields, value, t);
}

function getObjectSummaryFromFields(
  fields: ConfigField[],
  summaryFields: string[] | undefined,
  value: unknown,
  t: TFn,
): string {
  const objectValue =
    value && typeof value === "object" && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : {};
  const selectedFields = getObjectSummaryFields(fields, summaryFields);
  const summary = selectedFields
    .map((field): string => {
      const fieldValue = objectValue[field.key];
      if (field.timeRange?.role === "end") return "";
      if (field.timeRange?.role === "start") {
        const endField = findTimeRangePair(fields, field);
        const endValue = endField ? objectValue[endField.key] : undefined;
        if (isEmptyConfigValue(fieldValue) && isEmptyConfigValue(endValue)) {
          return t(WEBUI.plugins.unrestrictedTime);
        }
        if (
          typeof fieldValue === "string" &&
          typeof endValue === "string" &&
          fieldValue &&
          endValue
        ) {
          return fieldValue > endValue
            ? `${fieldValue}–${t(WEBUI.plugins.nextDay)} ${endValue}`
            : `${fieldValue}–${endValue}`;
        }
        return t(WEBUI.plugins.timeRangeIncomplete);
      }
      const formatted: string =
        field.type === "object"
          ? getObjectSummaryFromFields(
              field.fields ?? [],
              field.summaryFields,
              fieldValue,
              t,
            )
          : formatConfigFieldSummaryValue(field, fieldValue, t);
      return formatted ? `${field.label}: ${formatted}` : "";
    })
    .filter(Boolean)
    .join(" · ");

  return summary || t(WEBUI.plugins.notConfigured);
}

function formatConfigFieldSummaryValue(
  field: ConfigField,
  value: unknown,
  t: TFn,
) {
  if (
    field.type === "array" &&
    field.arrayPresentation &&
    Array.isArray(value) &&
    field.item &&
    "options" in field.item
  ) {
    const options = field.item.options ?? [];
    if (value.length === 0) {
      return field.arrayPresentation === "weekday-chips"
        ? t(WEBUI.plugins.everyDay)
        : t(WEBUI.plugins.unrestricted);
    }
    const selected = new Set(value.map((entry) => String(entry)));
    const labels = options
      .filter((option) => selected.has(String(option.value)))
      .map((option) => option.label);
    return labels.join(", ");
  }
  return formatSummaryValue(value, t);
}

function getObjectSummaryFields(
  fields: ConfigField[],
  summaryFields: string[] | undefined,
): ConfigField[] {
  const summaryKeys = summaryFields?.length
    ? summaryFields
    : [fields[0]?.key].filter(Boolean);
  return summaryKeys
    .map((key) => fields.find((field) => field.key === key))
    .filter((field): field is ConfigField => Boolean(field));
}

function formatSummaryValue(value: unknown, t: TFn): string {
  if (value === undefined || value === null || value === "") return "";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  if (Array.isArray(value)) {
    if (value.length === 0) return "";
    const primitiveValues = value.filter(
      (entry) =>
        typeof entry === "string" ||
        typeof entry === "number" ||
        typeof entry === "boolean",
    );
    if (primitiveValues.length === value.length) {
      return primitiveValues.map(String).join(", ");
    }
    return t(WEBUI.plugins.itemCount, { count: value.length });
  }
  if (typeof value === "object") {
    const values = Object.values(value)
      .map((v) => formatSummaryValue(v, t))
      .filter(Boolean);
    return values.join(" · ");
  }
  return "";
}

function inferArrayItemOption(
  value: unknown,
  options: ConfigFieldChild[],
): ConfigFieldChild {
  const normalized = stringifyConfigValue(value).trim();

  if (value && typeof value === "object" && !Array.isArray(value)) {
    const objectOption = options.find((option) => option.type === "object");
    if (objectOption) return objectOption;
  }

  if (
    (normalized.startsWith("$") || normalized.startsWith("!$")) &&
    options.some((option) => option.type === "reference")
  ) {
    return options.find((option) => option.type === "reference")!;
  }

  return options.find((option) => option.type !== "reference") ?? options[0];
}

function getChildOptionKey(item: ConfigFieldChild) {
  return item.optionKey ?? item.type;
}

function getChildLabel(item: ConfigFieldChild, t: TFn) {
  if (item.label) return item.label;
  if (item.type === "reference") return t(WEBUI.plugins.referenceLabel);
  if (item.type === "object") return t(WEBUI.plugins.objectLabel);
  return t(WEBUI.plugins.inputValueLabel);
}

function isEmptyConfigValue(value: unknown): boolean {
  if (value === undefined || value === null || value === "") return true;
  if (Array.isArray(value)) return value.length === 0;
  if (typeof value === "object") {
    const values = Object.values(value);
    return values.length === 0 || values.every(isEmptyConfigValue);
  }
  return false;
}

function createArrayItemFromString(value: string): ArrayItemValue {
  const normalized = value.trim();
  const withoutInvert = stripInvertPrefix(normalized);

  if (withoutInvert.startsWith("$")) {
    return {
      id: createArrayItemId(),
      syntax: "plugin",
      value: normalized,
      invert: normalized.startsWith("!"),
    };
  }

  return {
    id: createArrayItemId(),
    syntax: inferSyntaxFromValue(normalized),
    value: normalized,
  };
}

function serializeArrayItem(item: ArrayItemValue) {
  const trimmed = item.value.trim();
  if (!trimmed) return "";

  if (item.syntax === "plugin") {
    const tag = stripReferencePrefix(stripInvertPrefix(trimmed));
    if (!tag) return "";
    return `${item.invert ? "!" : ""}$${tag}`;
  }

  return trimmed;
}

function getSyntaxOptions(field: ConfigField): ArrayItemSyntax[] {
  const text =
    `${field.key} ${field.label} ${field.description ?? ""} ${getConfigFieldExample(field) ?? ""}`.toLowerCase();

  if (text.includes("provider 引用") || text.includes("只接受 $tag")) {
    return ["plugin"];
  }

  if (
    text.includes("matcher") ||
    text.includes("$tag") ||
    text.includes("quick setup")
  ) {
    return ["plugin", "quick", "value"];
  }

  if (
    text.includes("域名") ||
    text.includes("domain") ||
    text.includes("qname") ||
    text.includes("cname")
  ) {
    return ["domain", "plugin", "value"];
  }

  if (text.includes("ip") || text.includes("cidr")) {
    return ["value", "plugin"];
  }

  if (text.includes("文件") || text.includes("file")) {
    return ["value"];
  }

  return ["value"];
}

function inferDefaultSyntax(field: ConfigField): ArrayItemSyntax {
  return getSyntaxOptions(field)[0] ?? "value";
}

function inferSyntaxFromValue(value: string): ArrayItemSyntax {
  if (value.includes(":") && /^(full|domain|keyword|regexp):/.test(value)) {
    return "domain";
  }
  if (value.includes(" ")) return "quick";
  return "value";
}

function inferReferenceTypes(field: ConfigField): PluginType[] {
  const text =
    `${field.key} ${field.label} ${field.description ?? ""} ${getConfigFieldExample(field) ?? ""}`.toLowerCase();

  if (text.includes("executor")) return ["executor"];
  if (text.includes("matcher")) return ["matcher"];
  if (text.includes("provider")) return ["provider"];
  if (field.key === "sets" || field.key === "args")
    return ["provider", "matcher"];
  return ["provider", "matcher", "executor"];
}

function stringifyConfigValue(value: unknown) {
  return typeof value === "string" ? value : "";
}

function stripInvertPrefix(value: unknown) {
  const stringValue = stringifyConfigValue(value);
  return stringValue.startsWith("!") ? stringValue.slice(1) : stringValue;
}

function stripReferencePrefix(value: unknown) {
  const stringValue = stripInvertPrefix(value);
  return stringValue.startsWith("$") ? stringValue.slice(1) : stringValue;
}

function createArrayItemId() {
  return `item_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
}
