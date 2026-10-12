/*
 * SPDX-FileCopyrightText: 2023 shadcn
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later AND MIT
 * Derived from shadcn/ui; see LICENSE-shadcn in the WebUI root.
 */

import * as React from "react";

const MOBILE_BREAKPOINT = 768;
const QUERY = `(max-width: ${MOBILE_BREAKPOINT - 1}px)`;

function subscribe(callback: () => void) {
  const mql = window.matchMedia(QUERY);
  mql.addEventListener("change", callback);
  return () => mql.removeEventListener("change", callback);
}

function getSnapshot() {
  return window.matchMedia(QUERY).matches;
}

function getServerSnapshot() {
  return false;
}

export function useIsMobile() {
  return React.useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot);
}
