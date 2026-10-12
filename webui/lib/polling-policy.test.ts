/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { describe, expect, it } from "vitest";

import {
  ACTIVE_METRICS_POLL_INTERVAL_MS,
  SETTINGS_METRICS_POLL_INTERVAL_MS,
  metricsPollingInterval,
} from "./polling-policy";

describe("WebUI polling policy", () => {
  it("polls metrics at the active rate on metric-heavy routes", () => {
    expect(metricsPollingInterval("/", false)).toBe(
      ACTIVE_METRICS_POLL_INTERVAL_MS,
    );
    expect(metricsPollingInterval("/plugins", false)).toBe(
      ACTIVE_METRICS_POLL_INTERVAL_MS,
    );
  });

  it("uses a slower metrics rate for settings", () => {
    expect(metricsPollingInterval("/settings", false)).toBe(
      SETTINGS_METRICS_POLL_INTERVAL_MS,
    );
  });

  it("does not poll metrics where they are not rendered", () => {
    expect(metricsPollingInterval("/logs", false)).toBeNull();
    expect(metricsPollingInterval("/", true)).toBeNull();
    expect(metricsPollingInterval("/plugins", true)).toBeNull();
  });
});
