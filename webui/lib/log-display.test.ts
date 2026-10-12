/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { describe, expect, it } from "vitest";

import {
  DEFAULT_LOG_TIME_FORMAT,
  compactLogTarget,
  formatLogElapsed,
  formatLogTimestamp,
} from "./log-display";

describe("log timestamp display", () => {
  const timestamp = "2026-07-22T14:08:09.123456+08:00";

  it("shows only wall-clock time by default", () => {
    expect(formatLogTimestamp(timestamp)).toBe("14:08:09");
    expect(DEFAULT_LOG_TIME_FORMAT).toBe("HH:mm:ss");
  });

  it("supports date and millisecond tokens without timezone conversion", () => {
    expect(formatLogTimestamp(timestamp, "YYYY/MM/DD HH:mm:ss.SSS")).toBe(
      "2026/07/22 14:08:09.123",
    );
  });

  it("falls back to the original value for a non-ISO timestamp", () => {
    expect(formatLogTimestamp("not-a-timestamp")).toBe("not-a-timestamp");
  });
});

describe("log elapsed time display", () => {
  it("uses compact adaptive units", () => {
    expect(formatLogElapsed(426)).toBe("426ms");
    expect(formatLogElapsed(1_250)).toBe("1.250s");
    expect(formatLogElapsed(42_000)).toBe("42.000s");
    expect(formatLogElapsed(125_042)).toBe("2m05.042s");
    expect(formatLogElapsed(7_445_007)).toBe("2h04m05.007s");
  });
});

describe("log target compaction", () => {
  it("removes leading Rust module segments before touching the suffix", () => {
    expect(
      compactLogTarget(
        "oxidns::plugin::executor::forward::upstream::request",
        32,
      ),
    ).toBe("…::forward::upstream::request");
  });

  it("preserves the filename and nearest directories", () => {
    expect(
      compactLogTarget(
        "/workspace/src/plugin/executor/forward/upstream.rs",
        34,
      ),
    ).toBe("…/executor/forward/upstream.rs");
  });

  it("crops an individually overlong final segment from the front", () => {
    expect(
      compactLogTarget("very-long-generated-source-file-name.rs", 18),
    ).toBe("…urce-file-name.rs");
  });
});
