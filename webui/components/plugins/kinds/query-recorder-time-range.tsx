/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type PointerEvent,
  type KeyboardEvent,
} from "react";
import { Slider } from "radix-ui";
import {
  CalendarDays,
  ChevronLeft,
  ChevronRight,
  Focus,
  Info,
  MoveHorizontal,
  RefreshCw,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { DateTimeEditor } from "@/components/ui/datetime-picker";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useI18n } from "@/lib/i18n/provider";
import { WEBUI } from "@/lib/i18n/keys";
import { cn } from "@/lib/utils";
import {
  presetWindow,
  type AggregationWindow,
  type PresetRangeKey,
} from "./query-recorder-timeseries";
import {
  adjacentDayWindow,
  adjustTimelineWindow,
  createTimelineCommitter,
  fitTimeline,
  localTimestamp,
  editedTimestamp,
  sameInterval,
  shiftTimeline,
  syncTimeline,
  timelineStep,
  timelinePosition,
  timelineTimeAt,
  timelineTicks,
  timelineLabels,
  validateTimelineWindow,
  windowOnDate,
  type TimeInterval,
  type TimelineState,
  type TimelineTarget,
} from "./query-recorder-timeline";

// Keep the minute precision used by the existing record-list form.
export function localDateTime(ms: number) {
  return localTimestamp(ms).slice(0, 16);
}

function useRangeError() {
  const { t } = useI18n();
  return (error: ReturnType<typeof validateTimelineWindow>) =>
    error
      ? t(
          {
            missing: WEBUI.queryRecorder.rangeRequired,
            order: WEBUI.queryRecorder.rangeOrder,
            span: WEBUI.queryRecorder.rangeSpan,
            short: WEBUI.queryRecorder.rangeShort,
            future: WEBUI.queryRecorder.rangeFuture,
          }[error],
        )
      : null;
}

function TimeEndpoint({
  window,
  endpoint,
  onConfirm,
}: {
  window: TimeInterval;
  endpoint: "start" | "end";
  onConfirm: (window: TimeInterval) => void;
}) {
  const { t, formatDateTime } = useI18n();
  const rangeError = useRangeError();
  const [open, setOpen] = useState(false);
  const [input, setInput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const ms = endpoint === "start" ? window.sinceMs : window.untilMs;
  const label = t(
    endpoint === "start"
      ? WEBUI.queryRecorder.startTimeFilter
      : WEBUI.queryRecorder.endTimeFilter,
  );
  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        if (next) {
          setInput(localTimestamp(ms));
          setError(null);
        }
        setOpen(next);
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          className={cn(
            "min-w-0 rounded px-1 py-0.5 text-left text-xs hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring",
            endpoint === "end" && "text-right",
          )}
        >
          <span
            className={cn(
              "flex items-center gap-1 text-muted-foreground",
              endpoint === "end" && "justify-end",
            )}
          >
            {label}
            <CalendarDays className="size-3" />
          </span>
          <span className="font-mono">
            <span className="block sm:inline">
              {formatDateTime(ms, {
                year: "numeric",
                month: "2-digit",
                day: "2-digit",
              })}
            </span>{" "}
            <span className="block sm:inline">
              {formatDateTime(ms, {
                hour: "2-digit",
                minute: "2-digit",
                second: "2-digit",
                hour12: false,
              })}
            </span>
          </span>
        </button>
      </PopoverTrigger>
      <PopoverContent
        align={endpoint === "start" ? "start" : "end"}
        className="max-h-[var(--radix-popover-content-available-height)] w-72 max-w-[calc(100vw-2rem)] overflow-y-auto p-3"
      >
        <form
          className="space-y-3"
          onSubmit={(event) => {
            event.preventDefault();
            const next = {
              ...window,
              [endpoint === "start" ? "sinceMs" : "untilMs"]: editedTimestamp(
                input,
                ms,
              ),
            };
            const invalid = rangeError(
              validateTimelineWindow(next, Date.now()),
            );
            if (invalid) {
              setError(invalid);
              return;
            }
            onConfirm(next);
            setOpen(false);
          }}
        >
          <div className="flex items-center justify-between gap-2 text-xs">
            <span className="font-medium">{label}</span>
            <span className="font-mono tabular-nums text-muted-foreground">
              {input.replace("T", " ")}
            </span>
          </div>
          <DateTimeEditor
            value={input}
            precision="second"
            onChange={(next) => {
              setInput(next);
              setError(null);
            }}
          />
          {error && (
            <p role="alert" className="text-xs text-destructive">
              {error}
            </p>
          )}
          <div className="flex justify-end gap-1 border-t pt-2">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => setOpen(false)}
            >
              {t(WEBUI.common.cancel)}
            </Button>
            <Button type="submit" size="sm">
              {t(WEBUI.queryRecorder.confirmRange)}
            </Button>
          </div>
        </form>
      </PopoverContent>
    </Popover>
  );
}

function CalendarJump({
  window,
  onPick,
}: {
  window: TimeInterval;
  onPick: (day: Date) => void;
}) {
  const { t, formatDateTime } = useI18n();
  const [open, setOpen] = useState(false);
  const [month, setMonth] = useState(() => new Date(window.untilMs));
  const [now, setNow] = useState(Date.now);
  const first = new Date(month.getFullYear(), month.getMonth(), 1);
  first.setDate(1 - first.getDay());
  const days = Array.from({ length: 42 }, (_, index) => {
    const day = new Date(first);
    day.setDate(first.getDate() + index);
    return day;
  });
  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        if (next) {
          setMonth(new Date(window.untilMs));
          setNow(Date.now());
        }
        setOpen(next);
      }}
    >
      <PopoverTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          aria-label={t(WEBUI.queryRecorder.jumpDate)}
        >
          <CalendarDays className="size-3.5" />
          {formatDateTime(window.untilMs, { month: "2-digit", day: "2-digit" })}
        </Button>
      </PopoverTrigger>
      <PopoverContent
        className="w-auto max-w-[calc(100vw-2rem)] p-3"
        align="end"
      >
        <p className="max-w-60 text-xs text-muted-foreground">
          {t(WEBUI.queryRecorder.jumpDateHint)}
        </p>
        <div className="flex items-center justify-between gap-2">
          <Button
            variant="ghost"
            size="icon"
            aria-label={t(WEBUI.dateTime.previousMonth)}
            onClick={() =>
              setMonth(new Date(month.getFullYear(), month.getMonth() - 1, 1))
            }
          >
            <ChevronLeft />
          </Button>
          <span className="text-sm">
            {formatDateTime(month, { year: "numeric", month: "long" })}
          </span>
          <Button
            variant="ghost"
            size="icon"
            aria-label={t(WEBUI.dateTime.nextMonth)}
            onClick={() =>
              setMonth(new Date(month.getFullYear(), month.getMonth() + 1, 1))
            }
          >
            <ChevronRight />
          </Button>
        </div>
        <div className="grid grid-cols-7 gap-0.5">
          {Array.from({ length: 7 }, (_, index) => (
            <span
              key={index}
              className="grid h-7 place-items-center text-xs text-muted-foreground"
            >
              {formatDateTime(new Date(2024, 0, 7 + index), {
                weekday: "short",
              })}
            </span>
          ))}
          {days.map((day) => {
            const selected =
              day.toDateString() === new Date(window.untilMs).toDateString();
            const invalid =
              validateTimelineWindow(windowOnDate(window, day), now) !== null;
            return (
              <button
                key={day.getTime()}
                type="button"
                aria-label={formatDateTime(day, {
                  year: "numeric",
                  month: "long",
                  day: "numeric",
                })}
                aria-pressed={selected}
                disabled={invalid}
                onClick={() => {
                  onPick(day);
                  setOpen(false);
                }}
                className={cn(
                  "grid size-8 place-items-center rounded text-xs focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-30",
                  day.getMonth() !== month.getMonth() &&
                    "text-muted-foreground",
                  selected
                    ? "bg-primary text-primary-foreground"
                    : "hover:bg-muted",
                )}
              >
                {day.getDate()}
              </button>
            );
          })}
        </div>
      </PopoverContent>
    </Popover>
  );
}

function IconAction({
  label,
  children,
  onClick,
  disabled = false,
}: {
  label: string;
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="inline-flex">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label={label}
            onClick={onClick}
            disabled={disabled}
          >
            {children}
          </Button>
        </span>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

type Gesture = {
  kind: "pointer" | "keyboard";
  target: TimelineTarget;
  base: AggregationWindow;
  latest: AggregationWindow;
  viewport: TimeInterval;
  now: number;
  clientX: number;
  width: number;
  pointerId?: number;
};

export function QueryRecorderTimeRange({
  value,
  onChange,
  onRefresh,
  title,
  dataCoverage = [],
}: {
  value: AggregationWindow;
  onChange: (value: AggregationWindow) => void;
  onRefresh: () => void;
  title?: ReactNode;
  dataCoverage?: readonly TimeInterval[];
}) {
  const { t, formatDateTime } = useI18n();
  const [state, setState] = useState<TimelineState>(() => ({
    applied: value,
    draft: value,
    viewport: fitTimeline(value, Date.now()),
    adjusting: false,
  }));
  const [now, setNow] = useState(Date.now);
  const [width, setWidth] = useState(0);
  const [hoverPosition, setHoverPosition] = useState<number | null>(null);
  const trackRef = useRef<HTMLSpanElement>(null);
  const gestureRef = useRef<Gesture | null>(null);
  const committer = useMemo(
    () =>
      createTimelineCommitter((next) => {
        setState((current) => ({ ...current, adjusting: false }));
        onChange(next);
      }),
    [onChange],
  );
  // Synchronize external refreshes without resetting the viewport on local commits.
  if (state.applied !== value)
    setState(syncTimeline(state, value, Math.max(now, value.untilMs)));
  useEffect(() => {
    committer.cancel();
    gestureRef.current = null;
    return () => committer.cancel();
  }, [value, committer]);
  useEffect(() => {
    const element = trackRef.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) =>
      setWidth(entry.contentRect.width),
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const { draft, viewport, applied } = state;
  const span = viewport.untilMs - viewport.sinceMs;
  const shortSelection = (width * (draft.untilMs - draft.sinceMs)) / span < 64;
  const ticks = timelineTicks(viewport, width);
  const timeLabels = timelineLabels(viewport, width);
  const windowCenter = timelinePosition(
    (draft.sinceMs + draft.untilMs) / 2,
    viewport,
  );
  const hoverTime =
    hoverPosition === null
      ? null
      : Math.round(viewport.sinceMs + (span * hoverPosition) / 100);
  const visibleCoverage = dataCoverage.filter(
    (coverage) =>
      coverage.untilMs >= viewport.sinceMs &&
      coverage.sinceMs <= viewport.untilMs,
  );
  const presets: PresetRangeKey[] = ["10m", "1h", "24h"];
  const labels = {
    "10m": WEBUI.queryRecorder.rangePreset10m,
    "1h": WEBUI.queryRecorder.rangePreset1h,
    "24h": WEBUI.queryRecorder.rangePreset24h,
  };
  const duration = draft.untilMs - draft.sinceMs;
  const durationLabel = [
    Math.floor(duration / 3_600_000),
    Math.floor(duration / 60_000) % 60,
    Math.floor(duration / 1_000) % 60,
  ]
    .map((part) => String(part).padStart(2, "0"))
    .join(":");

  const submit = (next: AggregationWindow, nextViewport = viewport) => {
    gestureRef.current = null;
    setState((current) => ({
      ...current,
      draft: next,
      viewport: nextViewport,
      adjusting: !sameInterval(next, current.applied),
    }));
    committer.schedule(next, applied);
  };
  const confirm = (window: TimeInterval) => {
    if (sameInterval(window, draft)) return;
    const next = { ...draft, ...window, range: "custom" as const };
    const visible =
      window.sinceMs >= viewport.sinceMs && window.untilMs <= viewport.untilMs;
    submit(next, visible ? viewport : fitTimeline(window, Date.now()));
  };
  const jump = (window: TimeInterval) => {
    if (
      validateTimelineWindow(window, Date.now()) ||
      sameInterval(window, draft)
    )
      return;
    submit(
      { ...draft, ...window, range: "custom" },
      shiftTimeline(viewport, window.untilMs - draft.untilMs, Date.now()),
    );
  };
  const startPointer = (
    event: PointerEvent<HTMLElement>,
    target: TimelineTarget,
  ) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    committer.cancel();
    event.currentTarget.focus();
    event.currentTarget.setPointerCapture(event.pointerId);
    gestureRef.current = {
      kind: "pointer",
      target,
      base: draft,
      latest: draft,
      viewport,
      now: Date.now(),
      clientX: event.clientX,
      width: trackRef.current?.getBoundingClientRect().width || 1,
      pointerId: event.pointerId,
    };
    setState((current) => ({ ...current, adjusting: true }));
  };
  const movePointer = (event: PointerEvent<HTMLElement>) => {
    const gesture = gestureRef.current;
    if (gesture?.kind !== "pointer" || gesture.pointerId !== event.pointerId)
      return;
    const next = adjustTimelineWindow(
      gesture.base,
      gesture.target,
      ((event.clientX - gesture.clientX) / gesture.width) *
        (gesture.viewport.untilMs - gesture.viewport.sinceMs),
      gesture.viewport,
      gesture.now,
    );
    gesture.latest = { ...gesture.base, ...next, range: "custom" };
    setState((current) => ({ ...current, draft: gesture.latest }));
  };
  const finish = () => {
    const gesture = gestureRef.current;
    if (!gesture) return;
    submit(
      sameInterval(gesture.latest, gesture.base)
        ? gesture.base
        : gesture.latest,
      gesture.viewport,
    );
  };
  const cancel = () => {
    const gesture = gestureRef.current;
    if (!gesture) return;
    submit(gesture.base, gesture.viewport);
  };
  const keyDown = (
    event: KeyboardEvent<HTMLElement>,
    target: TimelineTarget,
  ) => {
    if (event.key === "Escape") {
      event.preventDefault();
      cancel();
      return;
    }
    const negative = event.key === "ArrowLeft" || event.key === "ArrowDown";
    const positive = event.key === "ArrowRight" || event.key === "ArrowUp";
    if (!negative && !positive && event.key !== "Home" && event.key !== "End")
      return;
    event.preventDefault();
    if (
      gestureRef.current?.kind !== "keyboard" ||
      gestureRef.current.target !== target
    ) {
      committer.cancel();
      gestureRef.current = {
        kind: "keyboard",
        target,
        base: draft,
        latest: draft,
        viewport,
        now: Date.now(),
        clientX: 0,
        width: 0,
      };
    }
    const gesture = gestureRef.current;
    const current = gesture.latest;
    const delta =
      event.key === "Home"
        ? -span
        : event.key === "End"
          ? span
          : (negative ? -1 : 1) *
            timelineStep(gesture.viewport) *
            (event.shiftKey ? 10 : 1);
    const next = adjustTimelineWindow(
      current,
      target,
      delta,
      gesture.viewport,
      gesture.now,
    );
    gesture.latest = { ...current, ...next, range: "custom" };
    setState((previous) => ({
      ...previous,
      draft: gesture.latest,
      adjusting: true,
    }));
  };
  const pointerUp = (event: PointerEvent<HTMLElement>) => {
    if (gestureRef.current?.pointerId === event.pointerId) finish();
  };
  const lostPointer = () => {
    if (gestureRef.current?.kind === "pointer") cancel();
  };
  const endKeyboard = () => {
    if (gestureRef.current?.kind === "keyboard") finish();
  };

  return (
    <div
      className="min-w-0 space-y-2"
      onFocusCapture={() => setNow(Date.now())}
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        {title && (
          <div className="flex items-center gap-2 text-sm font-medium">
            {title}
          </div>
        )}
        <div className="inline-flex items-center rounded-md border bg-muted/30 p-0.5">
          {presets.map((range) => (
            <button
              key={range}
              type="button"
              aria-pressed={draft.range === range}
              className={cn(
                "rounded px-2 py-1 text-xs focus-visible:ring-2 focus-visible:ring-ring",
                draft.range === range
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground",
              )}
              onClick={() => {
                committer.cancel();
                gestureRef.current = null;
                const next = presetWindow(range, Date.now(), applied.nonce + 1);
                setState((current) => ({
                  ...current,
                  draft: next,
                  viewport: fitTimeline(next, next.untilMs),
                  adjusting: false,
                }));
                onChange(next);
              }}
            >
              {t(labels[range])}
            </button>
          ))}
        </div>
        <div className="ml-auto flex flex-wrap items-center gap-0.5">
          <IconAction
            label={t(WEBUI.queryRecorder.previousDay)}
            disabled={
              validateTimelineWindow(adjacentDayWindow(draft, -1), now) !== null
            }
            onClick={() => jump(adjacentDayWindow(draft, -1))}
          >
            <ChevronLeft />
          </IconAction>
          <CalendarJump
            window={draft}
            onPick={(day) => jump(windowOnDate(draft, day))}
          />
          <IconAction
            label={t(WEBUI.queryRecorder.nextDay)}
            disabled={
              validateTimelineWindow(adjacentDayWindow(draft, 1), now) !== null
            }
            onClick={() => jump(adjacentDayWindow(draft, 1))}
          >
            <ChevronRight />
          </IconAction>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              const now = Date.now();
              jump({ sinceMs: now - duration, untilMs: now });
            }}
          >
            {t(WEBUI.queryRecorder.returnNow)}
          </Button>
          <IconAction
            label={t(WEBUI.queryRecorder.fitTimeline)}
            onClick={() =>
              setState((current) => ({
                ...current,
                viewport: fitTimeline(current.draft, Date.now()),
              }))
            }
          >
            <Focus />
          </IconAction>
          <IconAction
            label={t(WEBUI.common.refresh)}
            onClick={() => {
              committer.cancel();
              gestureRef.current = null;
              setState((current) => ({
                ...current,
                draft: current.applied,
                adjusting: false,
              }));
              onRefresh();
            }}
          >
            <RefreshCw />
          </IconAction>
        </div>
      </div>
      <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-1">
        <TimeEndpoint window={draft} endpoint="start" onConfirm={confirm} />
        <div className="px-2 text-center text-xs">
          <span className="block text-muted-foreground">
            {t(WEBUI.queryRecorder.windowDuration)}
          </span>
          <span className="font-mono tabular-nums">{durationLabel}</span>
        </div>
        <TimeEndpoint window={draft} endpoint="end" onConfirm={confirm} />
      </div>
      <div className={cn("relative mx-4", shortSelection ? "h-20" : "h-12")}>
        <Slider.Root
          ref={trackRef}
          min={viewport.sinceMs}
          max={viewport.untilMs}
          step={1}
          minStepsBetweenThumbs={1_000}
          value={[draft.sinceMs, draft.untilMs]}
          className={cn(
            "absolute inset-x-0 flex touch-none cursor-pointer items-center select-none",
            shortSelection ? "top-4 h-8" : "top-1 h-6",
          )}
          onPointerDown={(event) => event.preventDefault()}
          onPointerMove={(event) => {
            if (event.pointerType !== "mouse" && event.pointerType !== "pen")
              return;
            const rect = event.currentTarget.getBoundingClientRect();
            setHoverPosition(
              timelinePosition(
                timelineTimeAt(event.clientX, rect.left, rect.width, viewport),
                viewport,
              ),
            );
          }}
          onPointerLeave={() => setHoverPosition(null)}
        >
          <Slider.Track className="relative h-full w-full overflow-hidden rounded-md border border-primary/20 bg-primary/8">
            <Slider.Range className="absolute inset-y-0 rounded-sm border-y border-primary/40 bg-primary/12" />
            {visibleCoverage.map((coverage) => {
              const start = timelinePosition(coverage.sinceMs, viewport);
              const end = timelinePosition(coverage.untilMs, viewport);
              return (
                <span
                  key={`${coverage.sinceMs}-${coverage.untilMs}`}
                  aria-hidden="true"
                  className="pointer-events-none absolute inset-y-0 min-w-[2px] bg-primary/35"
                  style={{
                    left: `${start}%`,
                    width: `${end - start}%`,
                    ...(start === 100
                      ? { transform: "translateX(-100%)" }
                      : {}),
                  }}
                />
              );
            })}
            {ticks.map((tick) => (
              <span
                key={tick.timestamp}
                aria-hidden="true"
                className="pointer-events-none absolute top-0 h-2 w-px bg-primary/35"
                style={{ left: `${tick.position}%` }}
              />
            ))}
          </Slider.Track>
          <button
            type="button"
            className="absolute top-1/2 z-20 flex h-6 w-8 -translate-x-1/2 -translate-y-1/2 touch-none cursor-pointer items-center justify-center rounded border border-primary/35 bg-background/70 text-primary shadow-sm backdrop-blur-sm hover:bg-primary/15 focus-visible:ring-2 focus-visible:ring-ring active:cursor-grabbing"
            style={{ left: `${windowCenter}%` }}
            aria-label={t(WEBUI.queryRecorder.moveWindow)}
            title={t(WEBUI.queryRecorder.moveWindowHint)}
            aria-keyshortcuts="ArrowLeft ArrowRight ArrowUp ArrowDown"
            onPointerDown={(event) => startPointer(event, "window")}
            onPointerMove={movePointer}
            onPointerUp={pointerUp}
            onPointerCancel={cancel}
            onLostPointerCapture={lostPointer}
            onKeyDown={(event) => keyDown(event, "window")}
            onKeyUp={endKeyboard}
            onBlur={endKeyboard}
          >
            <MoveHorizontal className="size-4" />
          </button>
          {(["start", "end"] as const).map((target) => (
            <Slider.Thumb
              key={target}
              aria-label={t(
                target === "start"
                  ? WEBUI.queryRecorder.startTimeFilter
                  : WEBUI.queryRecorder.endTimeFilter,
              )}
              aria-valuetext={formatDateTime(
                target === "start" ? draft.sinceMs : draft.untilMs,
                {
                  year: "numeric",
                  month: "2-digit",
                  day: "2-digit",
                  hour: "2-digit",
                  minute: "2-digit",
                  second: "2-digit",
                  hour12: false,
                },
              )}
              className="group relative z-10 block size-0 cursor-ew-resize outline-none"
              onPointerDown={(event) => startPointer(event, target)}
              onPointerMove={movePointer}
              onPointerUp={pointerUp}
              onPointerCancel={cancel}
              onLostPointerCapture={lostPointer}
              onKeyDown={(event) => keyDown(event, target)}
              onKeyUp={endKeyboard}
              onBlur={endKeyboard}
            >
              <span
                className={cn(
                  "absolute left-0 flex w-8 -translate-x-1/2 items-center justify-center rounded group-focus-visible:ring-2 group-focus-visible:ring-ring",
                  shortSelection
                    ? target === "start"
                      ? "-top-8 h-8"
                      : "top-0 h-8"
                    : "-top-4 h-8",
                )}
              >
                <span
                  className={cn(
                    "flex w-3 items-center justify-center rounded-sm border border-primary/60 bg-primary/30 shadow-sm",
                    shortSelection ? "h-4" : "h-6",
                  )}
                >
                  <span className="h-3 w-px bg-primary/80" />
                </span>
              </span>
              {shortSelection && (
                <span className="pointer-events-none absolute -top-4 left-0 h-8 w-px bg-primary/40" />
              )}
            </Slider.Thumb>
          ))}
          {hoverPosition !== null && hoverTime !== null && (
            <div
              aria-hidden="true"
              className="pointer-events-none absolute inset-y-0 z-30 w-px bg-primary/70"
              style={{ left: `${hoverPosition}%` }}
            >
              <span
                className="absolute top-1 whitespace-nowrap rounded border bg-popover px-2 py-1 font-mono text-[10px] text-popover-foreground shadow-sm"
                style={{
                  transform:
                    hoverPosition < 33
                      ? "none"
                      : hoverPosition > 66
                        ? "translateX(-100%)"
                        : "translateX(-50%)",
                }}
              >
                {formatDateTime(hoverTime, {
                  year: "numeric",
                  month: "2-digit",
                  day: "2-digit",
                  hour: "2-digit",
                  minute: "2-digit",
                  second: "2-digit",
                  hour12: false,
                })}
              </span>
            </div>
          )}
        </Slider.Root>
        <div className="pointer-events-none absolute inset-x-0 bottom-0 h-4 text-[10px] text-muted-foreground">
          {timeLabels.map((tick) => (
            <span
              key={tick.timestamp}
              className="absolute whitespace-nowrap"
              style={{
                left: `${tick.position}%`,
                transform:
                  tick.position < 10
                    ? "none"
                    : tick.position > 90
                      ? "translateX(-100%)"
                      : "translateX(-50%)",
              }}
            >
              {formatDateTime(tick.timestamp, {
                ...(span > 86_400_000
                  ? { month: "2-digit", day: "2-digit" }
                  : {}),
                hour: "2-digit",
                minute: "2-digit",
                hour12: false,
              })}
            </span>
          ))}
        </div>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-1 text-xs text-muted-foreground">
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              className="inline-flex items-center gap-1 rounded focus-visible:ring-2 focus-visible:ring-ring"
            >
              <Info className="size-3" />
              {t(WEBUI.queryRecorder.aggregationLimitCompact)}
            </button>
          </TooltipTrigger>
          <TooltipContent>
            {t(WEBUI.queryRecorder.aggregationLimit)}
          </TooltipContent>
        </Tooltip>
        <div className="flex flex-wrap items-center gap-2">
          <span className="inline-flex items-center gap-1">
            <span
              aria-hidden="true"
              className="h-2.5 w-3 rounded-sm border border-primary/30 bg-primary/12"
            />
            {t(WEBUI.queryRecorder.selectedWindow)}
          </span>
          {visibleCoverage.length > 0 && (
            <span className="inline-flex items-center gap-1">
              <span
                aria-hidden="true"
                className="h-2.5 w-3 rounded-sm bg-primary/35"
              />
              {t(WEBUI.queryRecorder.timelineDataCoverage)}
            </span>
          )}
        </div>
        <span role="status" aria-live="polite">
          {state.adjusting ? t(WEBUI.queryRecorder.adjustingRange) : ""}
        </span>
      </div>
    </div>
  );
}
