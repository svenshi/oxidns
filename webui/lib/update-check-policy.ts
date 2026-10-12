/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

export const AUTO_UPDATE_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1_000;
export const FAILED_UPDATE_CHECK_RETRY_INTERVAL_MS = 60 * 60 * 1_000;

export interface UpdateCheckContext {
  backend: string;
  currentVersion: string;
  repository: string;
  bundle: string;
  allowPrerelease: boolean;
  requestOptionsFingerprint: string;
}

export interface UpdateCheckAttempt {
  requestKey: string;
  checkedAt: number;
  succeeded: boolean;
}

export function updateCheckRequestKey(context: UpdateCheckContext): string {
  return JSON.stringify(context);
}

/** Create a non-reversible cache discriminator without persisting credentials. */
export function updateCheckOptionsFingerprint(values: string[]): string {
  const input = JSON.stringify(values);
  let hash = 0x811c9dc5;
  for (let index = 0; index < input.length; index += 1) {
    hash ^= input.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return `${(hash >>> 0).toString(16).padStart(8, "0")}:${input.length}`;
}

export function isAutomaticUpdateCheckDue(
  previous: UpdateCheckAttempt | null,
  requestKey: string,
  now = Date.now(),
): boolean {
  if (!previous || previous.requestKey !== requestKey) return true;
  const interval = previous.succeeded
    ? AUTO_UPDATE_CHECK_INTERVAL_MS
    : FAILED_UPDATE_CHECK_RETRY_INTERVAL_MS;
  return now - previous.checkedAt >= interval;
}
