"use client";

import * as React from "react";
import { CalendarDays, ChevronLeft, ChevronRight, X } from "lucide-react";

import { cn } from "@/lib/utils";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

function pad(value: number) {
  return value.toString().padStart(2, "0");
}

// Parse local timestamps with optional second precision.
function parseValue(value: string): Date | null {
  if (!value) return null;
  const match = value.match(
    /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/,
  );
  if (!match) return null;
  const [, y, mo, d, h, mi, s] = match;
  const date = new Date(
    Number(y),
    Number(mo) - 1,
    Number(d),
    Number(h),
    Number(mi),
    Number(s ?? 0),
  );
  return Number.isNaN(date.getTime()) ? null : date;
}

// Serializes a Date back into the `datetime-local` string the form expects.
function formatValue(
  date: Date,
  precision: "minute" | "second" = "minute",
): string {
  const value = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(
    date.getDate(),
  )}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
  return precision === "second" ? `${value}:${pad(date.getSeconds())}` : value;
}

function sameDay(a: Date, b: Date) {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

interface DateTimeEditorProps {
  value: string;
  onChange: (value: string) => void;
  precision?: "minute" | "second";
  showActions?: boolean;
  className?: string;
}

function DateTimeEditor({
  value,
  onChange,
  precision = "minute",
  showActions = false,
  className,
}: DateTimeEditorProps) {
  const { formatDateTime, t } = useI18n();
  const selected = React.useMemo(() => parseValue(value), [value]);

  // The month currently shown in the calendar grid; tracks the selected
  // value but stays independent so navigation does not mutate the filter.
  const [viewDate, setViewDate] = React.useState<Date>(
    () => selected ?? new Date(),
  );

  const weekdays = React.useMemo(
    () =>
      Array.from({ length: 7 }, (_, index) =>
        formatDateTime(new Date(2024, 0, 7 + index), { weekday: "short" }),
      ),
    [formatDateTime],
  );
  const monthLabel = formatDateTime(viewDate, {
    year: "numeric",
    month: "long",
  });
  const commit = React.useCallback(
    (next: Date) => {
      onChange(formatValue(next, precision));
    },
    [onChange, precision],
  );

  const handleDayClick = (day: Date) => {
    const base = selected ?? new Date(day);
    const next = new Date(
      day.getFullYear(),
      day.getMonth(),
      day.getDate(),
      selected ? base.getHours() : 0,
      selected ? base.getMinutes() : 0,
      selected ? base.getSeconds() : 0,
    );
    commit(next);
  };

  const handleTimeChange = (part: "h" | "m" | "s", raw: string) => {
    const digits = raw.replace(/\D/g, "");
    if (!digits) return;
    const num = Number(digits);
    const base = selected ?? viewDate;
    const next = new Date(
      base.getFullYear(),
      base.getMonth(),
      base.getDate(),
      part === "h" ? Math.min(23, Math.max(0, num)) : base.getHours(),
      part === "m" ? Math.min(59, Math.max(0, num)) : base.getMinutes(),
      part === "s" ? Math.min(59, Math.max(0, num)) : base.getSeconds(),
    );
    commit(next);
  };

  const goToToday = () => {
    const now = new Date();
    setViewDate(now);
    commit(now);
  };

  const monthStart = new Date(viewDate.getFullYear(), viewDate.getMonth(), 1);
  const gridStart = new Date(monthStart);
  gridStart.setDate(1 - monthStart.getDay());

  const days = Array.from({ length: 42 }, (_, index) => {
    const day = new Date(gridStart);
    day.setDate(gridStart.getDate() + index);
    return day;
  });

  const today = new Date();

  return (
    <div className={cn("flex flex-col gap-3", className)}>
      <div className="flex items-center justify-between">
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label={t(WEBUI.dateTime.previousMonth)}
          onClick={() =>
            setViewDate(
              new Date(viewDate.getFullYear(), viewDate.getMonth() - 1, 1),
            )
          }
        >
          <ChevronLeft />
        </Button>
        <span className="text-sm font-medium">{monthLabel}</span>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label={t(WEBUI.dateTime.nextMonth)}
          onClick={() =>
            setViewDate(
              new Date(viewDate.getFullYear(), viewDate.getMonth() + 1, 1),
            )
          }
        >
          <ChevronRight />
        </Button>
      </div>

      <div className="grid grid-cols-7 gap-0.5">
        {weekdays.map((label) => (
          <div
            key={label}
            className="grid h-7 place-items-center text-xs text-muted-foreground"
          >
            {label}
          </div>
        ))}
        {days.map((day) => {
          const inMonth = day.getMonth() === viewDate.getMonth();
          const isSelected = selected ? sameDay(day, selected) : false;
          const isToday = sameDay(day, today);
          return (
            <button
              type="button"
              key={day.toISOString()}
              aria-label={formatDateTime(day, {
                year: "numeric",
                month: "long",
                day: "numeric",
              })}
              aria-pressed={isSelected}
              onClick={() => handleDayClick(day)}
              className={cn(
                "grid h-7 w-full place-items-center rounded-md font-mono text-xs transition-colors focus-visible:ring-2 focus-visible:ring-ring",
                !inMonth && "text-muted-foreground/40",
                inMonth && !isSelected && "hover:bg-muted",
                isSelected &&
                  "bg-primary font-medium text-primary-foreground hover:bg-primary/85",
                !isSelected && isToday && "ring-1 ring-inset ring-primary/40",
              )}
            >
              {day.getDate()}
            </button>
          );
        })}
      </div>

      <div className="flex items-center justify-between border-t border-border pt-2.5">
        <span className="text-xs text-muted-foreground">
          {t(WEBUI.dateTime.time)}
        </span>
        <div className="flex items-center gap-1 font-mono text-sm">
          {(
            [
              {
                part: "h",
                label: WEBUI.dateTime.hour,
                value: selected?.getHours(),
                max: 23,
              },
              {
                part: "m",
                label: WEBUI.dateTime.minute,
                value: selected?.getMinutes(),
                max: 59,
              },
              ...(precision === "second"
                ? [
                    {
                      part: "s" as const,
                      label: WEBUI.dateTime.second,
                      value: selected?.getSeconds(),
                      max: 59,
                    },
                  ]
                : []),
            ] as const
          ).map((field, index) => (
            <React.Fragment key={field.part}>
              {index > 0 && <span className="text-muted-foreground">:</span>}
              <input
                inputMode="numeric"
                aria-label={t(field.label)}
                title={t(field.label)}
                value={field.value == null ? "--" : pad(field.value)}
                onFocus={(event) => event.currentTarget.select()}
                onChange={(event) =>
                  handleTimeChange(field.part, event.target.value)
                }
                onKeyDown={(event) => {
                  if (event.key !== "ArrowUp" && event.key !== "ArrowDown")
                    return;
                  event.preventDefault();
                  const next = Math.min(
                    field.max,
                    Math.max(
                      0,
                      (field.value ?? 0) + (event.key === "ArrowUp" ? 1 : -1),
                    ),
                  );
                  handleTimeChange(field.part, String(next));
                }}
                className="h-8 w-10 rounded-md border border-input bg-muted/30 text-center outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/50"
              />
            </React.Fragment>
          ))}
        </div>
      </div>

      {showActions && (
        <div className="flex items-center justify-between">
          <Button
            type="button"
            variant="ghost"
            size="xs"
            onClick={() => onChange("")}
          >
            {t(WEBUI.common.clear)}
          </Button>
          <Button type="button" variant="ghost" size="xs" onClick={goToToday}>
            {t(WEBUI.dateTime.now)}
          </Button>
        </div>
      )}
    </div>
  );
}

interface DateTimePickerProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  className?: string;
}

function DateTimePicker({
  value,
  onChange,
  placeholder,
  className,
}: DateTimePickerProps) {
  const { formatDateTime, t } = useI18n();
  const [open, setOpen] = React.useState(false);
  const selected = React.useMemo(() => parseValue(value), [value]);
  const selectedLabel = selected
    ? formatDateTime(selected, {
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
      })
    : (placeholder ?? t(WEBUI.dateTime.placeholder));
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          data-slot="datetime-trigger"
          className={cn(
            "flex h-8 w-full min-w-0 items-center gap-2 rounded-lg border border-input bg-transparent px-2.5 py-1 text-left text-sm transition-colors outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 aria-expanded:border-ring dark:bg-input/30",
            className,
          )}
        >
          <CalendarDays className="size-3.5 shrink-0 text-muted-foreground" />
          <span
            className={cn(
              "flex-1 truncate font-mono",
              !selected && "text-muted-foreground",
            )}
          >
            {selectedLabel}
          </span>
          {selected && (
            <span
              role="button"
              tabIndex={-1}
              aria-label={t(WEBUI.common.clear)}
              onClick={(event) => {
                event.stopPropagation();
                onChange("");
              }}
              className="grid size-4 shrink-0 place-items-center rounded text-muted-foreground hover:bg-muted hover:text-foreground"
            >
              <X className="size-3" />
            </span>
          )}
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        className="max-h-[var(--radix-popover-content-available-height)] w-64 max-w-[calc(100vw-2rem)] overflow-y-auto p-3"
      >
        <DateTimeEditor value={value} onChange={onChange} showActions />
      </PopoverContent>
    </Popover>
  );
}

export { DateTimePicker, DateTimeEditor };
