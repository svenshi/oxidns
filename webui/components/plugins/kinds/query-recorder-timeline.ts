import {
  MAX_AGGREGATION_SPAN_MS,
  validateAggregationRange,
  type AggregationWindow,
} from "./query-recorder-timeseries";

export interface TimeInterval {
  sinceMs: number;
  untilMs: number;
}

export type TimelineTarget = "start" | "end" | "window";
export const MIN_WINDOW_MS = 1_000;
export const TIMELINE_COMMIT_DELAY_MS = 300;
const HOUR_MS = 3_600_000;
const clamp = (value: number, min: number, max: number) =>
  Math.min(max, Math.max(min, value));

export function sameInterval(a: TimeInterval, b: TimeInterval) {
  return a.sinceMs === b.sinceMs && a.untilMs === b.untilMs;
}

export function fitTimeline(window: TimeInterval, now: number): TimeInterval {
  const span = Math.min(
    now,
    clamp((window.untilMs - window.sinceMs) * 3, HOUR_MS, 72 * HOUR_MS),
  );
  const start = clamp(
    (window.sinceMs + window.untilMs - span) / 2,
    0,
    now - span,
  );
  return { sinceMs: Math.floor(start), untilMs: Math.floor(start + span) };
}

export function shiftTimeline(
  viewport: TimeInterval,
  deltaMs: number,
  now: number,
): TimeInterval {
  const span = Math.min(viewport.untilMs - viewport.sinceMs, now);
  const start = clamp(viewport.sinceMs + deltaMs, 0, now - span);
  return { sinceMs: start, untilMs: start + span };
}

export function timelineStep(viewport: TimeInterval) {
  const span = viewport.untilMs - viewport.sinceMs;
  return span <= HOUR_MS ? 1_000 : span <= 6 * HOUR_MS ? 10_000 : 60_000;
}

export function adjustTimelineWindow(
  window: TimeInterval,
  target: TimelineTarget,
  deltaMs: number,
  viewport: TimeInterval,
  now: number,
): TimeInterval {
  const step = timelineStep(viewport);
  const delta = Math.round(deltaMs / step) * step;
  const min = Math.max(0, viewport.sinceMs);
  const max = Math.min(now, viewport.untilMs);
  if (target === "window") {
    const shift = clamp(delta, min - window.sinceMs, max - window.untilMs);
    return { sinceMs: window.sinceMs + shift, untilMs: window.untilMs + shift };
  }
  if (target === "start") {
    return {
      sinceMs: clamp(
        window.sinceMs + delta,
        Math.max(min, window.untilMs - MAX_AGGREGATION_SPAN_MS),
        window.untilMs - MIN_WINDOW_MS,
      ),
      untilMs: window.untilMs,
    };
  }
  return {
    sinceMs: window.sinceMs,
    untilMs: clamp(
      window.untilMs + delta,
      window.sinceMs + MIN_WINDOW_MS,
      Math.min(max, window.sinceMs + MAX_AGGREGATION_SPAN_MS),
    ),
  };
}

export function validateTimelineWindow(window: TimeInterval, now: number) {
  return (
    validateAggregationRange(window.sinceMs, window.untilMs) ??
    (window.untilMs - window.sinceMs < MIN_WINDOW_MS
      ? "short"
      : window.untilMs > now
        ? "future"
        : null)
  );
}

// Calendar navigation keeps the local end time and the elapsed duration, including DST.
export function windowOnDate(window: TimeInterval, day: Date): TimeInterval {
  const end = new Date(window.untilMs);
  const untilMs = new Date(
    day.getFullYear(),
    day.getMonth(),
    day.getDate(),
    end.getHours(),
    end.getMinutes(),
    end.getSeconds(),
    end.getMilliseconds(),
  ).getTime();
  const shifted = new Date(untilMs);
  // A local clock time in a DST gap cannot be preserved on the target date.
  if (
    shifted.getFullYear() !== day.getFullYear() ||
    shifted.getMonth() !== day.getMonth() ||
    shifted.getDate() !== day.getDate() ||
    shifted.getHours() !== end.getHours() ||
    shifted.getMinutes() !== end.getMinutes()
  ) {
    return { sinceMs: NaN, untilMs: NaN };
  }
  return { sinceMs: untilMs - (window.untilMs - window.sinceMs), untilMs };
}

export function adjacentDayWindow(window: TimeInterval, days: number) {
  const day = new Date(window.untilMs);
  day.setDate(day.getDate() + days);
  return windowOnDate(window, day);
}

export function localTimestamp(ms: number) {
  const date = new Date(ms);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

export function parseLocalTimestamp(value: string): number {
  const match = value.match(
    /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/,
  );
  if (!match) return NaN;
  const [year, month, day, hour, minute, second] = match.slice(1).map(Number);
  const date = new Date(year, month - 1, day, hour, minute, second || 0);
  // Reject normalized invalid dates/times instead of silently changing the selection.
  return date.getFullYear() === year &&
    date.getMonth() === month - 1 &&
    date.getDate() === day &&
    date.getHours() === hour &&
    date.getMinutes() === minute &&
    date.getSeconds() === (second || 0)
    ? date.getTime()
    : NaN;
}

export function editedTimestamp(input: string, previous: number) {
  const normalized = input.length === 16 ? `${input}:00` : input;
  return normalized === localTimestamp(previous)
    ? previous
    : parseLocalTimestamp(input);
}

export interface TimelineState {
  applied: AggregationWindow;
  draft: AggregationWindow;
  viewport: TimeInterval;
  adjusting: boolean;
}

export function syncTimeline(
  state: TimelineState,
  applied: AggregationWindow,
  now: number,
): TimelineState {
  const sameDuration =
    applied.untilMs - applied.sinceMs ===
    state.applied.untilMs - state.applied.sinceMs;
  const viewport = sameInterval(state.draft, applied)
    ? state.viewport
    : sameDuration
      ? shiftTimeline(
          state.viewport,
          applied.untilMs - state.applied.untilMs,
          now,
        )
      : fitTimeline(applied, now);
  // Refresh may discard a draft that was fitted outside the applied interval.
  const containsApplied =
    applied.sinceMs >= viewport.sinceMs && applied.untilMs <= viewport.untilMs;
  return {
    applied,
    draft: applied,
    viewport: containsApplied ? viewport : fitTimeline(applied, now),
    adjusting: false,
  };
}

export function createTimelineCommitter(
  onCommit: (window: AggregationWindow) => void,
) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const cancel = () => {
    clearTimeout(timer);
    timer = undefined;
  };
  return {
    cancel,
    schedule(window: AggregationWindow, applied: AggregationWindow) {
      cancel();
      if (sameInterval(window, applied)) return;
      timer = setTimeout(() => {
        timer = undefined;
        onCommit({ ...window, range: "custom", nonce: applied.nonce + 1 });
      }, TIMELINE_COMMIT_DELAY_MS);
    },
  };
}

export function timelinePosition(timestamp: number, viewport: TimeInterval) {
  return clamp(
    ((timestamp - viewport.sinceMs) / (viewport.untilMs - viewport.sinceMs)) *
      100,
    0,
    100,
  );
}

export function timelineTimeAt(
  clientX: number,
  left: number,
  width: number,
  viewport: TimeInterval,
) {
  const ratio = clamp((clientX - left) / Math.max(1, width), 0, 1);
  return Math.round(
    viewport.sinceMs + ratio * (viewport.untilMs - viewport.sinceMs),
  );
}

const RULER_INTERVALS_MS = [
  1_000,
  5_000,
  10_000,
  15_000,
  30_000,
  60_000,
  120_000,
  300_000,
  600_000,
  900_000,
  1_800_000,
  HOUR_MS,
  2 * HOUR_MS,
  3 * HOUR_MS,
  6 * HOUR_MS,
  12 * HOUR_MS,
  24 * HOUR_MS,
  48 * HOUR_MS,
];

function timelineRulerSteps(viewport: TimeInterval, width: number) {
  const span = viewport.untilMs - viewport.sinceMs;
  const labelCount = clamp(Math.floor(width / 100), 2, 8);
  const labelStep =
    RULER_INTERVALS_MS.find((interval) => interval >= span / labelCount) ??
    48 * HOUR_MS;
  // Use familiar intervals that divide the label spacing so both rulers align.
  const tickStep =
    RULER_INTERVALS_MS.findLast(
      (interval) => interval <= labelStep / 3 && labelStep % interval === 0,
    ) ?? RULER_INTERVALS_MS[0];
  return { labelStep, tickStep };
}

function alignedTimelineMarks(viewport: TimeInterval, step: number) {
  const midnight = new Date(viewport.sinceMs);
  midnight.setHours(0, 0, 0, 0);
  const anchor = midnight.getTime();
  const first = Math.ceil((viewport.sinceMs - anchor) / step);
  const last = Math.floor((viewport.untilMs - anchor) / step);
  return Array.from({ length: Math.max(0, last - first + 1) }, (_, offset) => {
    const timestamp = anchor + (first + offset) * step;
    return { timestamp, position: timelinePosition(timestamp, viewport) };
  });
}

export function timelineTicks(viewport: TimeInterval, width: number) {
  return alignedTimelineMarks(
    viewport,
    timelineRulerSteps(viewport, width).tickStep,
  );
}

export function timelineLabels(viewport: TimeInterval, width: number) {
  return alignedTimelineMarks(
    viewport,
    timelineRulerSteps(viewport, width).labelStep,
  );
}
