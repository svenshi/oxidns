/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { WEBUI } from "./i18n";

export const MAX_PLUGIN_TAG_LENGTH = 64;

const RESERVED_PLUGIN_TAG_PREFIXES = ["qs.exec.", "qs.match.", "qs.cron."];

export type PluginTagValidationError =
  | "empty"
  | "too_long"
  | "non_ascii"
  | "invalid_character"
  | "empty_segment"
  | "invalid_segment_boundary";

/**
 * Validate the readable, path-safe plugin tag grammar shared with the Rust
 * configuration validator. Tags are a single API path segment, so dots are
 * allowed only as separators between non-empty, alphanumeric-bounded parts.
 */
export function validatePluginTag(
  tag: string,
): PluginTagValidationError | null {
  if (!tag) return "empty";
  if (new TextEncoder().encode(tag).length > MAX_PLUGIN_TAG_LENGTH) {
    return "too_long";
  }
  if (!/^[\x00-\x7F]+$/.test(tag)) return "non_ascii";
  if (!/^[A-Za-z0-9_.-]+$/.test(tag)) return "invalid_character";

  for (const segment of tag.split(".")) {
    if (!segment) return "empty_segment";
    if (!/^[A-Za-z0-9].*[A-Za-z0-9]$/.test(segment) && !/^[A-Za-z0-9]$/.test(segment)) {
      return "invalid_segment_boundary";
    }
  }

  return null;
}

export function isValidPluginTag(tag: string): boolean {
  return validatePluginTag(tag) === null;
}

export function isReservedPluginTag(tag: string): boolean {
  return RESERVED_PLUGIN_TAG_PREFIXES.some((prefix) => tag.startsWith(prefix));
}

export function pluginTagValidationMessageKey(error: PluginTagValidationError) {
  switch (error) {
    case "empty":
      return WEBUI.storeErrors.pluginNameRequired;
    case "too_long":
      return WEBUI.storeErrors.pluginNameTooLong;
    case "non_ascii":
      return WEBUI.storeErrors.pluginNameNonAscii;
    case "invalid_character":
      return WEBUI.storeErrors.pluginNameInvalidCharacter;
    case "empty_segment":
      return WEBUI.storeErrors.pluginNameEmptySegment;
    case "invalid_segment_boundary":
      return WEBUI.storeErrors.pluginNameInvalidSegmentBoundary;
  }
}
