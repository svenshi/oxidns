/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import type { PluginCardComponentProps } from "./types";
import { PluginCardTemplate } from "./plugin-card-template";
import { getPluginCatalogItem } from "./catalog";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import type { ConfigField } from "@/lib/plugin-definitions";
import { PluginCardItemGrid } from "./plugin-card-item-grid";

const MAX_CARD_CONFIG_ITEMS = 6;
const SENSITIVE_CONFIG_KEY =
  /(?:password|passwd|secret|token|api[_-]?key|private[_-]?key)/i;

export function DefaultPluginCard(props: PluginCardComponentProps) {
  const { locale, t } = useI18n();
  const definition = getPluginCatalogItem(props.plugin.pluginKind, locale);
  const configFields = selectCardConfigFields(
    definition?.configSchema ?? [],
    props.plugin.config,
  );
  const configItems = configFields.map((field) => ({
    key: field.key,
    label: field.label,
    value: formatCardConfigValue(
      field,
      props.plugin.config[field.key] === undefined
        ? field.default
        : props.plugin.config[field.key],
      t,
    ),
  }));

  return (
    <PluginCardTemplate {...props}>
      {configItems.length > 0 ? (
        <PluginCardItemGrid items={configItems} />
      ) : null}
    </PluginCardTemplate>
  );
}

export function selectCardConfigFields(
  fields: ConfigField[],
  config: Record<string, unknown>,
  limit = MAX_CARD_CONFIG_ITEMS,
): ConfigField[] {
  const visibleFields = fields.filter(
    (field) => !field.advanced && isCardConfigField(field),
  );
  const configuredAdvancedFields = fields.filter(
    (field) =>
      field.advanced &&
      isCardConfigField(field) &&
      hasCardConfigValue(config[field.key]),
  );

  const prioritizeFields = (candidates: ConfigField[]) =>
    candidates
      .map((field, index) => ({
        field,
        index,
        required: Boolean(field.required),
        configured: hasCardConfigValue(config[field.key]),
      }))
      .sort(
        (left, right) =>
          Number(right.required) - Number(left.required) ||
          Number(right.configured) - Number(left.configured) ||
          left.index - right.index,
      )
      .map(({ field }) => field);

  const primary = prioritizeFields(visibleFields).slice(0, limit);
  if (primary.length === limit) return primary;

  return [
    ...primary,
    ...prioritizeFields(configuredAdvancedFields).slice(
      0,
      limit - primary.length,
    ),
  ];
}

function isCardConfigField(field: ConfigField): boolean {
  if (field.type === "password" || field.type === "textarea") return false;
  if (field.type === "json") return false;
  return !SENSITIVE_CONFIG_KEY.test(field.key);
}

function hasCardConfigValue(value: unknown): boolean {
  if (value === undefined || value === null || value === "") return false;
  if (Array.isArray(value)) return value.length > 0;
  if (typeof value === "object") return Object.keys(value).length > 0;
  return true;
}

function formatCardConfigValue(
  field: ConfigField,
  value: unknown,
  t: ReturnType<typeof useI18n>["t"],
) {
  if (value === undefined || value === null || value === "") {
    return t(WEBUI.common.unconfigured);
  }
  if (typeof value === "boolean") {
    return value ? t(WEBUI.common.yes) : t(WEBUI.common.no);
  }
  if (typeof value === "number") return String(value);
  if (typeof value === "string") {
    const option = field.options?.find(
      (candidate) =>
        candidate.value === value || candidate.aliases?.includes(value),
    );
    return option?.label ?? value;
  }
  if (Array.isArray(value))
    return value.length > 0
      ? t(WEBUI.common.itemCount, { count: value.length })
      : t(WEBUI.common.empty);
  if (typeof value === "object") {
    const count = Object.keys(value).length;
    return count > 0
      ? t(WEBUI.common.itemCount, { count })
      : t(WEBUI.common.empty);
  }
  return String(value);
}
