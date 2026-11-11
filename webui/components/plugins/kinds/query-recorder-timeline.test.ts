/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import {
  presetWindow,
  refreshWindow,
  type AggregationWindow,
} from "./query-recorder-timeseries";
import {
  adjacentDayWindow,
  adjustTimelineWindow,
  createTimelineCommitter,
  editedTimestamp,
  fitTimeline,
  localTimestamp,
  parseLocalTimestamp,
  shiftTimeline,
  syncTimeline,
  timelineStep,
  timelinePosition,
  timelineTimeAt,
  timelineTicks,
  timelineLabels,
  validateTimelineWindow,
  windowOnDate,
  type TimelineState,
} from "./query-recorder-timeline";

const HOUR = 3_600_000;
const DAY = 24 * HOUR;
const NOW = 100 * DAY;
const interval = (sinceMs: number, untilMs: number) => ({ sinceMs, untilMs });
const selection = (sinceMs: number, untilMs: number): AggregationWindow => ({
  ...interval(sinceMs, untilMs),
  range: "custom",
  nonce: 7,
});

afterEach(() => vi.useRealTimers());

describe("timeline scale and stable viewport", () => {
  it.each([
    [10 * 60_000, HOUR],
    [HOUR, 3 * HOUR],
    [DAY, 3 * DAY],
  ])("fits %s ms without showing future time", (duration, visibleSpan) => {
    const view = fitTimeline(interval(NOW - duration, NOW), NOW);
    expect(view).toEqual(interval(NOW - visibleSpan, NOW));
  });
  it("centers historical windows and stays within the epoch boundary", () => {
    expect(fitTimeline(interval(10 * DAY, 10 * DAY + HOUR), NOW)).toEqual(
      interval(10 * DAY - HOUR, 10 * DAY + 2 * HOUR),
    );
    expect(fitTimeline(interval(0, HOUR), NOW)).toEqual(interval(0, 3 * HOUR));
  });
  it("preserves the viewport after a local endpoint commit", () => {
    const applied = presetWindow("1h", NOW, 7);
    const viewport = fitTimeline(applied, NOW);
    const draft = selection(NOW - 30 * 60_000, NOW);
    const state: TimelineState = { applied, draft, viewport, adjusting: true };
    const synced = syncTimeline(state, { ...draft, nonce: 8 }, NOW);
    expect(synced.viewport).toBe(viewport);
    expect(synced.adjusting).toBe(false);
  });
  it("shifts the viewport by the same amount as a recent refresh", () => {
    const applied = presetWindow("1h", NOW);
    const viewport = fitTimeline(applied, NOW);
    const state: TimelineState = {
      applied,
      draft: applied,
      viewport,
      adjusting: false,
    };
    expect(
      syncTimeline(state, refreshWindow(applied, NOW + 15_000), NOW + 15_000)
        .viewport,
    ).toEqual(interval(viewport.sinceMs + 15_000, viewport.untilMs + 15_000));
  });
  it("restores a visible applied window when refresh cancels an offscreen fitted draft", () => {
    const applied = presetWindow("1h", NOW);
    const draft = selection(NOW - 3 * HOUR, NOW - 2 * HOUR);
    const viewport = fitTimeline(draft, NOW);
    const next = refreshWindow(applied, NOW + 15_000);
    const synced = syncTimeline(
      { applied, draft, viewport, adjusting: true },
      next,
      NOW + 15_000,
    );
    expect(synced.draft).toBe(next);
    expect(synced.viewport).toEqual(fitTimeline(next, NOW + 15_000));
  });
  it("keeps historical refresh bounds and discards uncommitted edits", () => {
    const applied = selection(NOW - 2 * HOUR, NOW - HOUR);
    const viewport = fitTimeline(applied, NOW);
    const draft = selection(NOW - 3 * HOUR, NOW - HOUR);
    const next = refreshWindow(applied, NOW + HOUR);
    expect(
      syncTimeline(
        { applied, draft, viewport, adjusting: true },
        next,
        NOW + HOUR,
      ),
    ).toEqual({ applied: next, draft: next, viewport, adjusting: false });
  });
  it("translates historical scales without changing their width, clamping at now", () => {
    const view = interval(NOW - 5 * HOUR, NOW - 2 * HOUR);
    expect(shiftTimeline(view, -DAY, NOW)).toEqual(
      interval(view.sinceMs - DAY, view.untilMs - DAY),
    );
    expect(shiftTimeline(view, 10 * HOUR, NOW)).toEqual(
      interval(NOW - 3 * HOUR, NOW),
    );
  });
  it.each([
    [HOUR, 1_000],
    [HOUR + 1, 10_000],
    [6 * HOUR, 10_000],
    [6 * HOUR + 1, 60_000],
  ])("uses the appropriate step for %s ms", (span, step) => {
    expect(timelineStep(interval(0, span))).toBe(step);
  });
});

describe("endpoint and whole-window adjustment", () => {
  const viewport = interval(NOW - 3 * DAY, NOW);
  const window = interval(NOW - 2 * HOUR, NOW - HOUR);
  it("clamps crossing endpoints to a one-second interval", () => {
    expect(
      adjustTimelineWindow(window, "start", 10 * HOUR, viewport, NOW),
    ).toEqual(interval(window.untilMs - 1_000, window.untilMs));
    expect(
      adjustTimelineWindow(window, "end", -10 * HOUR, viewport, NOW),
    ).toEqual(interval(window.sinceMs, window.sinceMs + 1_000));
  });
  it("never expands either endpoint past 24 hours", () => {
    const old = interval(NOW - DAY, NOW - 23 * HOUR);
    expect(
      adjustTimelineWindow(old, "end", 3 * DAY, viewport, NOW).untilMs -
        old.sinceMs,
    ).toBe(DAY);
    expect(
      adjustTimelineWindow(window, "start", -3 * DAY, viewport, NOW).sinceMs,
    ).toBe(window.untilMs - DAY);
  });
  it("keeps duration while clamping whole-window movement at visible bounds", () => {
    expect(adjustTimelineWindow(window, "window", DAY, viewport, NOW)).toEqual(
      interval(NOW - HOUR, NOW),
    );
    expect(
      adjustTimelineWindow(window, "window", -4 * DAY, viewport, NOW),
    ).toEqual(interval(viewport.sinceMs, viewport.sinceMs + HOUR));
  });
  it("does not exceed now even when a viewport is stale", () => {
    const stale = interval(NOW - HOUR, NOW + HOUR);
    expect(
      adjustTimelineWindow(interval(NOW - 60_000, NOW), "end", HOUR, stale, NOW)
        .untilMs,
    ).toBe(NOW);
  });
  it("snaps movement relative to the original timestamps, preserving milliseconds", () => {
    const view = interval(NOW - HOUR, NOW);
    const start = interval(NOW - 600_000 + 123, NOW - 100_000 + 123);
    expect(adjustTimelineWindow(start, "window", -1_700, view, NOW)).toEqual(
      interval(start.sinceMs - 2_000, start.untilMs - 2_000),
    );
  });
  it.each([
    [interval(NaN, NOW), "missing"],
    [interval(0, 0), "order"],
    [interval(NOW, NOW - 1), "order"],
    [interval(NOW - DAY - 1, NOW), "span"],
    [interval(NOW - 999, NOW), "short"],
    [interval(NOW - 1_000, NOW), null],
    [interval(NOW - DAY, NOW), null],
    [interval(NOW, NOW + 1_000), "future"],
  ])("validates bounds before submission", (window, error) => {
    expect(
      validateTimelineWindow(
        window as { sinceMs: number; untilMs: number },
        NOW,
      ),
    ).toBe(error);
  });
});

describe("local date navigation and second precision", () => {
  it("moves a cross-midnight window while preserving the end clock and duration", () => {
    const end = new Date(2026, 9, 10, 0, 15, 27, 123).getTime();
    const window = interval(end - HOUR, end);
    const expectedEnd = new Date(2026, 9, 9, 0, 15, 27, 123).getTime();
    expect(adjacentDayWindow(window, -1)).toEqual(
      interval(expectedEnd - HOUR, expectedEnd),
    );
    expect(windowOnDate(window, new Date(2026, 9, 9))).toEqual(
      adjacentDayWindow(window, -1),
    );
  });
  it("preserves elapsed duration across daylight-saving transitions", () => {
    const previousZone = process.env.TZ;
    process.env.TZ = "America/New_York";
    try {
      const end = new Date(2026, 2, 7, 12, 30).getTime();
      const shifted = adjacentDayWindow(interval(end - DAY, end), 1);
      expect(shifted.untilMs - shifted.sinceMs).toBe(DAY);
      expect(shifted.untilMs - end).toBe(23 * HOUR);
      expect(new Date(shifted.untilMs).getHours()).toBe(12);
      expect(new Date(shifted.untilMs).getMinutes()).toBe(30);
    } finally {
      if (previousZone === undefined) delete process.env.TZ;
      else process.env.TZ = previousZone;
    }
  });
  it("rejects a date whose end clock falls in a daylight-saving gap", () => {
    const previousZone = process.env.TZ;
    process.env.TZ = "America/New_York";
    try {
      const end = new Date(2026, 2, 7, 2, 30).getTime();
      const shifted = adjacentDayWindow(interval(end - HOUR, end), 1);
      expect(validateTimelineWindow(shifted, NOW)).toBe("missing");
    } finally {
      if (previousZone === undefined) delete process.env.TZ;
      else process.env.TZ = previousZone;
    }
  });
  it("preserves hidden milliseconds when an unchanged timestamp is confirmed", () => {
    const timestamp = new Date(2026, 9, 9, 12, 34, 0, 123).getTime();
    expect(editedTimestamp(localTimestamp(timestamp), timestamp)).toBe(
      timestamp,
    );
    expect(editedTimestamp("2026-10-09T12:34", timestamp)).toBe(timestamp);
    expect(editedTimestamp("2026-10-09T12:34:01", timestamp)).toBe(
      new Date(2026, 9, 9, 12, 34, 1).getTime(),
    );
  });
  it("round-trips seconds without converting to UTC", () => {
    const timestamp = new Date(2026, 9, 9, 12, 34, 56).getTime();
    expect(localTimestamp(timestamp)).toBe("2026-10-09T12:34:56");
    expect(parseLocalTimestamp(localTimestamp(timestamp))).toBe(timestamp);
  });
  it.each([
    "",
    "2026-02-30T12:00:00",
    "2026-01-01T24:01:00",
    "2026-01-01T12:60:00",
    "invalid",
  ])("rejects invalid timestamps: %s", (value) => {
    expect(parseLocalTimestamp(value)).toBeNaN();
  });
});

describe("query submission budget", () => {
  it("does not submit a draft while dragging and commits once after release", () => {
    vi.useFakeTimers();
    const onCommit = vi.fn();
    const committer = createTimelineCommitter(onCommit);
    const applied = presetWindow("1h", NOW, 7);
    const viewport = fitTimeline(applied, NOW);
    const draft = {
      ...applied,
      ...adjustTimelineWindow(applied, "start", 60_000, viewport, NOW),
    };
    vi.advanceTimersByTime(1_000);
    expect(onCommit).not.toHaveBeenCalled();
    committer.schedule(draft, applied);
    vi.advanceTimersByTime(299);
    expect(onCommit).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onCommit).toHaveBeenCalledExactlyOnceWith({
      ...draft,
      range: "custom",
      nonce: 8,
    });
  });
  it("coalesces successive date or keyboard changes", () => {
    vi.useFakeTimers();
    const onCommit = vi.fn();
    const committer = createTimelineCommitter(onCommit);
    const applied = selection(NOW - HOUR, NOW);
    const first = { ...applied, ...adjacentDayWindow(applied, -1) };
    const second = { ...first, ...adjacentDayWindow(first, -1) };
    committer.schedule(first, applied);
    vi.advanceTimersByTime(200);
    committer.schedule(second, applied);
    vi.advanceTimersByTime(299);
    expect(onCommit).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onCommit).toHaveBeenCalledExactlyOnceWith({ ...second, nonce: 8 });
  });
  it("does not query unchanged windows, canceled operations, or pending edits after refresh/unmount", () => {
    vi.useFakeTimers();
    const onCommit = vi.fn();
    const committer = createTimelineCommitter(onCommit);
    const applied = selection(NOW - HOUR, NOW);
    committer.schedule(applied, applied);
    vi.runAllTimers();
    const draft = { ...applied, sinceMs: applied.sinceMs + 1_000 };
    committer.schedule(draft, applied);
    committer.schedule(applied, applied);
    vi.runAllTimers();
    committer.schedule(draft, applied);
    committer.cancel();
    vi.runAllTimers();
    expect(onCommit).not.toHaveBeenCalled();
  });
});

describe("timeline ruler and pointer time", () => {
  it("maps pointer position to the displayed time and clamps outside the track", () => {
    const viewport = interval(60_000, 3_660_000);
    expect(timelineTimeAt(100, 100, 400, viewport)).toBe(viewport.sinceMs);
    expect(timelineTimeAt(300, 100, 400, viewport)).toBe(1_860_000);
    expect(timelineTimeAt(500, 100, 400, viewport)).toBe(viewport.untilMs);
    expect(timelineTimeAt(90, 100, 400, viewport)).toBe(viewport.sinceMs);
    expect(timelineTimeAt(510, 100, 400, viewport)).toBe(viewport.untilMs);
    expect(timelinePosition(1_860_000, viewport)).toBe(50);
    expect(timelinePosition(0, viewport)).toBe(0);
    expect(timelinePosition(10_000_000, viewport)).toBe(100);
  });
  it.each([HOUR, 3 * HOUR, 72 * HOUR])(
    "adapts %s ms rulers to available width while keeping labels aligned",
    (span) => {
      const viewport = interval(NOW - span + 123, NOW + 123);
      for (const width of [280, 640, 1280]) {
        const ticks = timelineTicks(viewport, width);
        const labels = timelineLabels(viewport, width);
        expect(ticks.length).toBeGreaterThan(labels.length);
        expect(ticks.length).toBeLessThanOrEqual(40);
        expect(labels.length).toBeGreaterThan(0);
        expect(labels.length).toBeLessThanOrEqual(8);
        const tickStep = ticks[1].timestamp - ticks[0].timestamp;
        expect(ticks[0].timestamp - viewport.sinceMs).toBeLessThan(tickStep);
        expect(viewport.untilMs - ticks.at(-1)!.timestamp).toBeLessThan(
          tickStep,
        );
        for (let index = 1; index < ticks.length; index++) {
          expect(ticks[index].timestamp - ticks[index - 1].timestamp).toBe(
            tickStep,
          );
        }
        for (const tick of [...ticks, ...labels]) {
          expect(tick.timestamp).toBeGreaterThanOrEqual(viewport.sinceMs);
          expect(tick.timestamp).toBeLessThanOrEqual(viewport.untilMs);
          expect(tick.position).toBe(
            timelinePosition(tick.timestamp, viewport),
          );
        }
        for (const label of labels) {
          expect(ticks).toContainEqual(label);
        }
      }
    },
  );
  it.each([
    [HOUR, 120_000, 600_000],
    [3 * HOUR, 600_000, 1_800_000],
    [72 * HOUR, 3 * HOUR, 12 * HOUR],
  ])(
    "uses suitable tick and label intervals for %s ms",
    (span, tickStep, labelStep) => {
      const viewport = interval(NOW - span, NOW);
      const ticks = timelineTicks(viewport, 640);
      const labels = timelineLabels(viewport, 640);
      expect(ticks[1].timestamp - ticks[0].timestamp).toBe(tickStep);
      expect(labels[1].timestamp - labels[0].timestamp).toBe(labelStep);
    },
  );
});
