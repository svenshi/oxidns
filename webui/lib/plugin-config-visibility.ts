/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import type {
  ConfigField,
  ConfigFieldChild,
} from "./plugin-definitions/shared";

/** Resolve display-only conditions without changing the schema used for serialization. */
export function getVisiblePluginConfigFields(
  fields: ConfigField[],
  values: Record<string, unknown>,
): ConfigField[] {
  const visibleChild = (child: ConfigFieldChild): ConfigFieldChild => {
    if (child.type === "object") {
      return { ...child, fields: visibleFields(child.fields) };
    }
    if (child.type === "array" && child.item) {
      return { ...child, item: visibleChild(child.item) };
    }
    return child;
  };

  const visibleFields = (nestedFields: ConfigField[]): ConfigField[] =>
    nestedFields
      .filter((field) => {
        if (!field.visibleWhen) return true;
        const condition = field.visibleWhen;
        const value =
          values[condition.field] === undefined
            ? fields.find((root) => root.key === condition.field)?.default
            : values[condition.field];
        return value === condition.equals;
      })
      .map((field) => ({
        ...field,
        fields: field.fields ? visibleFields(field.fields) : undefined,
        item: field.item ? visibleChild(field.item) : undefined,
        itemOptions: field.itemOptions?.map(visibleChild),
      }));

  return visibleFields(fields);
}
