/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

export interface CacheSelection {
  ids: Set<string>;
  allMatching: boolean;
}
export type CacheSelectionAction =
  | { type: "clear" }
  | { type: "allMatching" }
  | { type: "loaded"; ids: string[]; checked: boolean }
  | { type: "toggle"; id: string; checked: boolean };

export function cacheSelectionReducer(
  state: CacheSelection,
  action: CacheSelectionAction,
): CacheSelection {
  if (action.type === "clear") return { ids: new Set(), allMatching: false };
  if (action.type === "allMatching")
    return { ids: new Set(), allMatching: true };
  if (action.type === "loaded")
    return {
      ids: new Set(action.checked ? action.ids : []),
      allMatching: false,
    };
  if (state.allMatching) return state;
  const ids = new Set(state.ids);
  if (action.checked) ids.add(action.id);
  else ids.delete(action.id);
  return { ids, allMatching: false };
}

export function mergeCacheEntries<T extends { id: string }>(
  current: T[],
  next: T[],
): T[] {
  return Array.from(
    new Map([...current, ...next].map((entry) => [entry.id, entry])).values(),
  );
}

export class CacheEntriesRequests {
  private controller?: AbortController;
  start(): AbortController {
    this.cancel();
    this.controller = new AbortController();
    return this.controller;
  }
  cancel(): void {
    this.controller?.abort();
  }
}
