/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

export const DEFAULT_LOG_TIME_FORMAT = "HH:mm:ss";

export const LOG_TIME_FORMAT_PRESETS = [
  DEFAULT_LOG_TIME_FORMAT,
  "HH:mm:ss.SSS",
  "YYYY-MM-DD HH:mm:ss",
  "YYYY-MM-DD HH:mm:ss.SSS",
] as const;

const LOG_TIMESTAMP_PATTERN =
  /^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?/;
const LOG_TIME_TOKEN_PATTERN = /YYYY|SSS|MM|DD|HH|mm|ss/g;

/**
 * Format the backend's local ISO-8601 wall clock without converting it into
 * the browser timezone. Unknown text is preserved as a literal separator.
 */
export function formatLogTimestamp(
  timestamp: string,
  format = DEFAULT_LOG_TIME_FORMAT,
): string {
  const match = LOG_TIMESTAMP_PATTERN.exec(timestamp);
  if (!match) return timestamp;

  const values: Record<string, string> = {
    YYYY: match[1],
    MM: match[2],
    DD: match[3],
    HH: match[4],
    mm: match[5],
    ss: match[6],
    SSS: (match[7] ?? "").padEnd(3, "0").slice(0, 3),
  };
  const normalizedFormat = format.trim() || DEFAULT_LOG_TIME_FORMAT;
  return normalizedFormat.replace(
    LOG_TIME_TOKEN_PATTERN,
    (token) => values[token] ?? token,
  );
}

/** Keep process elapsed time compact while retaining useful short-term detail. */
export function formatLogElapsed(elapsedMs: number): string {
  const milliseconds = Math.max(0, Math.round(elapsedMs));
  if (milliseconds < 1_000) return `${milliseconds}ms`;

  if (milliseconds < 60_000) {
    return `${(milliseconds / 1_000).toFixed(3)}s`;
  }

  const totalSeconds = Math.floor(milliseconds / 1_000);
  const seconds = totalSeconds % 60;
  const millisecondRemainder = milliseconds % 1_000;
  const preciseSeconds = `${String(seconds).padStart(2, "0")}.${String(
    millisecondRemainder,
  ).padStart(3, "0")}s`;
  const totalMinutes = Math.floor(totalSeconds / 60);
  if (totalMinutes < 60) return `${totalMinutes}m${preciseSeconds}`;

  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  return `${hours}h${String(minutes).padStart(2, "0")}m${preciseSeconds}`;
}

/**
 * Keep the most useful suffix of a source path or Rust module target. Whole
 * leading segments are removed first; only an individually overlong final
 * segment is cropped character-by-character.
 */
export function compactLogTarget(target: string, maxLength = 36): string {
  if (target.length <= maxLength || maxLength < 2) return target;

  const separator = target.includes("::")
    ? "::"
    : target.includes("/")
      ? "/"
      : target.includes("\\")
        ? "\\"
        : null;
  if (!separator) return `…${target.slice(-(maxLength - 1))}`;

  const parts = target.split(separator).filter(Boolean);
  if (parts.length === 0) return `…${target.slice(-(maxLength - 1))}`;

  let suffix = parts.at(-1) ?? target;
  if (suffix.length + 1 > maxLength) {
    return `…${suffix.slice(-(maxLength - 1))}`;
  }

  for (let index = parts.length - 2; index >= 0; index -= 1) {
    const candidate = `${parts[index]}${separator}${suffix}`;
    const prefixed = `…${separator}${candidate}`;
    if (prefixed.length > maxLength) break;
    suffix = candidate;
  }

  return `…${separator}${suffix}`;
}
