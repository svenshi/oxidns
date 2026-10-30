"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { Clock3, Pause, Play, Trash2, WifiOff, WrapText } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import {
  DEFAULT_LOG_TIME_FORMAT,
  LOG_TIME_FORMAT_PRESETS,
  compactLogTarget,
  formatLogElapsed,
  formatLogTimestamp,
} from "@/lib/log-display";
import { streamLogs, type LogEntry } from "@/lib/oxidns-api";

const LEVEL_COLORS: Record<
  string,
  { dot: string; badge: string; text: string }
> = {
  ERROR: {
    dot: "bg-red-500",
    badge: "bg-red-500/15 text-red-400 border-red-500/20",
    text: "text-red-400",
  },
  WARN: {
    dot: "bg-amber-400",
    badge: "bg-amber-400/15 text-amber-400 border-amber-400/20",
    text: "text-amber-400",
  },
  INFO: {
    dot: "bg-emerald-500",
    badge: "bg-emerald-500/15 text-emerald-400 border-emerald-500/20",
    text: "text-emerald-400",
  },
  DEBUG: {
    dot: "bg-sky-500",
    badge: "bg-sky-500/15 text-sky-400 border-sky-500/20",
    text: "text-sky-400",
  },
  TRACE: {
    dot: "bg-gray-500",
    badge: "bg-gray-500/15 text-gray-400 border-gray-500/20",
    text: "text-gray-500",
  },
};

const MAX_ENTRIES = 2000;
const LOG_DISPLAY_STORAGE_KEY = "oxidns:log-display";

interface LogDisplaySettings {
  timeFormat: string;
  showElapsed: boolean;
}

const DEFAULT_LOG_DISPLAY_SETTINGS: LogDisplaySettings = {
  timeFormat: DEFAULT_LOG_TIME_FORMAT,
  showElapsed: false,
};

function loadLogDisplaySettings(): LogDisplaySettings {
  if (typeof window === "undefined") return DEFAULT_LOG_DISPLAY_SETTINGS;
  try {
    const parsed = JSON.parse(
      localStorage.getItem(LOG_DISPLAY_STORAGE_KEY) ?? "null",
    ) as Partial<LogDisplaySettings> | null;
    return {
      timeFormat:
        typeof parsed?.timeFormat === "string" && parsed.timeFormat.trim()
          ? parsed.timeFormat.slice(0, 64)
          : DEFAULT_LOG_TIME_FORMAT,
      showElapsed:
        typeof parsed?.showElapsed === "boolean" ? parsed.showElapsed : false,
    };
  } catch {
    return DEFAULT_LOG_DISPLAY_SETTINGS;
  }
}

function LevelBadge({ level }: { level: string }) {
  const colors = LEVEL_COLORS[level] ?? LEVEL_COLORS.INFO;
  return (
    <Badge
      variant="outline"
      className={`shrink-0 font-mono text-[10px] px-1 py-0 h-4 w-[42px] justify-center ${colors.badge}`}
    >
      {level}
    </Badge>
  );
}

function LogLine({
  entry,
  wrap,
  timeFormat,
  showElapsed,
}: {
  entry: LogEntry;
  wrap: boolean;
  timeFormat: string;
  showElapsed: boolean;
}) {
  const colors = LEVEL_COLORS[entry.level] ?? LEVEL_COLORS.INFO;
  const wallClock = formatLogTimestamp(entry.timestamp, timeFormat);
  const target = compactLogTarget(entry.target);
  // When wrap is on: row fills the viewport width, message wraps inside the
  // remaining flex space. When off: row grows to its content width and the
  // viewport scrolls horizontally — preserves the prior dense layout.
  const rowClass = wrap
    ? "flex items-baseline gap-2 rounded px-1 py-[1px] hover:bg-white/5"
    : "flex min-w-full w-max items-baseline gap-2 rounded px-1 py-[1px] whitespace-nowrap hover:bg-white/5";
  const messageClass = wrap
    ? `${colors.text} flex-1 min-w-0 whitespace-pre-wrap break-all`
    : `${colors.text} shrink-0`;
  return (
    <div className={rowClass}>
      <span className="shrink-0 text-zinc-500 tabular-nums">{wallClock}</span>
      {showElapsed ? (
        <span className="shrink-0 text-zinc-600 tabular-nums">
          T+{formatLogElapsed(entry.elapsed_ms)}
        </span>
      ) : null}
      <LevelBadge level={entry.level} />
      <span
        className="max-w-[36ch] shrink-0 truncate text-zinc-500"
        title={entry.target}
      >
        {target}
      </span>
      <span className={messageClass}>{entry.message}</span>
    </div>
  );
}

export function LogViewer() {
  const { t } = useI18n();
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const [levelFilter, setLevelFilter] = useState<string>("all");
  const [search, setSearch] = useState("");
  const [paused, setPaused] = useState(false);
  const [connected, setConnected] = useState(false);
  const [backlog, setBacklog] = useState(0);
  const [wrap, setWrap] = useState(true);
  const [displaySettings, setDisplaySettings] = useState(
    loadLogDisplaySettings,
  );

  const scrollRef = useRef<HTMLDivElement>(null);
  const pausedRef = useRef(false);
  const pendingRef = useRef<LogEntry[]>([]);

  useEffect(() => {
    try {
      localStorage.setItem(
        LOG_DISPLAY_STORAGE_KEY,
        JSON.stringify(displaySettings),
      );
    } catch {
      // Display preferences remain active for this session when storage fails.
    }
  }, [displaySettings]);

  // keep pausedRef in sync so the streaming callback sees current value
  useEffect(() => {
    pausedRef.current = paused;
  }, [paused]);

  // auto-scroll to bottom when entries update and not paused
  useEffect(() => {
    if (!paused && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [entries, paused]);

  // connect SSE stream; reconnect with exponential backoff on disconnect
  useEffect(() => {
    let mounted = true;
    const controller = new AbortController();
    let retryDelay = 1500;

    const run = async () => {
      while (mounted && !controller.signal.aborted) {
        try {
          setConnected(true);
          await streamLogs(
            {
              level: levelFilter !== "all" ? levelFilter : undefined,
              tail: 200,
            },
            (entry) => {
              if (pausedRef.current) {
                pendingRef.current.push(entry);
                setBacklog((b) => b + 1);
              } else {
                setEntries((prev) => {
                  const next = [...prev, entry];
                  return next.length > MAX_ENTRIES
                    ? next.slice(-MAX_ENTRIES)
                    : next;
                });
              }
            },
            controller.signal,
          );
        } catch {
          if (controller.signal.aborted) break;
        }
        if (!mounted || controller.signal.aborted) break;
        setConnected(false);
        await new Promise<void>((resolve) => setTimeout(resolve, retryDelay));
        retryDelay = Math.min(retryDelay * 2, 30_000);
      }
    };

    run();
    return () => {
      mounted = false;
      controller.abort();
      setConnected(false);
      // clear stale entries so the next stream's tail doesn't produce duplicate keys
      setEntries([]);
      pendingRef.current = [];
      setBacklog(0);
    };
  }, [levelFilter]);

  const togglePause = useCallback(() => {
    setPaused((prev) => {
      if (prev) {
        // resume: flush pending entries
        const pending = pendingRef.current;
        pendingRef.current = [];
        setBacklog(0);
        setEntries((current) => {
          const next = [...current, ...pending];
          return next.length > MAX_ENTRIES ? next.slice(-MAX_ENTRIES) : next;
        });
      }
      return !prev;
    });
  }, []);

  const clearLogs = useCallback(() => {
    setEntries([]);
    pendingRef.current = [];
    setBacklog(0);
  }, []);

  const filtered = entries.filter((e) => {
    if (search) {
      const q = search.toLowerCase();
      if (
        !e.message.toLowerCase().includes(q) &&
        !e.target.toLowerCase().includes(q)
      ) {
        return false;
      }
    }
    return true;
  });

  return (
    <div className="flex flex-1 flex-col min-h-0 w-full">
      {/* Toolbar */}
      <div className="flex items-center gap-2 px-3 py-2 border-b shrink-0 flex-wrap">
        {/* Connection status */}
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground shrink-0">
          {connected ? (
            <span className="relative flex size-2">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-75" />
              <span className="relative inline-flex size-2 rounded-full bg-emerald-500" />
            </span>
          ) : (
            <WifiOff className="size-3 text-rose-500" />
          )}
          <span>
            {connected ? t(WEBUI.logs.connected) : t(WEBUI.logs.disconnected)}
          </span>
        </div>

        <div className="h-4 w-px bg-border shrink-0" />

        {/* Level filter */}
        <Select value={levelFilter} onValueChange={setLevelFilter}>
          <SelectTrigger className="h-7 w-28 text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent position="popper" sideOffset={4}>
            <SelectItem value="all">{t(WEBUI.logs.allLevels)}</SelectItem>
            <SelectItem value="ERROR">ERROR+</SelectItem>
            <SelectItem value="WARN">WARN+</SelectItem>
            <SelectItem value="INFO">INFO+</SelectItem>
            <SelectItem value="DEBUG">DEBUG+</SelectItem>
            <SelectItem value="TRACE">TRACE</SelectItem>
          </SelectContent>
        </Select>

        {/* Search */}
        <Input
          className="h-7 w-32 sm:w-48 text-xs"
          placeholder={t(WEBUI.logs.searchPlaceholder)}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />

        <Popover>
          <PopoverTrigger asChild>
            <Button variant="outline" size="sm" className="h-7 px-2 text-xs">
              <Clock3 className="mr-1 size-3" />
              {t(WEBUI.logs.timeFormat)}
            </Button>
          </PopoverTrigger>
          <PopoverContent align="start" className="w-[calc(100vw-2rem)] sm:w-[22rem] gap-2">
            <PopoverHeader className="gap-0.5">
              <PopoverTitle className="text-[13px] leading-5">
                {t(WEBUI.logs.timeFormat)}
              </PopoverTitle>
              <PopoverDescription className="text-xs leading-[1.45]">
                {t(WEBUI.logs.timeFormatDescription)}
              </PopoverDescription>
            </PopoverHeader>
            <div className="grid grid-cols-2 gap-1.5">
              {LOG_TIME_FORMAT_PRESETS.map((preset) => (
                <Button
                  key={preset}
                  variant={
                    displaySettings.timeFormat === preset
                      ? "secondary"
                      : "outline"
                  }
                  size="xs"
                  className="justify-start px-2 font-mono text-[11px]"
                  onClick={() =>
                    setDisplaySettings((current) => ({
                      ...current,
                      timeFormat: preset,
                    }))
                  }
                >
                  {preset}
                </Button>
              ))}
            </div>
            <Input
              className="h-7 font-mono text-[11px]"
              aria-label={t(WEBUI.logs.timeFormat)}
              maxLength={64}
              value={displaySettings.timeFormat}
              onChange={(event) =>
                setDisplaySettings((current) => ({
                  ...current,
                  timeFormat: event.target.value,
                }))
              }
            />
            <p className="font-mono text-[11px] text-muted-foreground">
              {t(WEBUI.logs.timeFormatPreview, {
                value: formatLogTimestamp(
                  "2026-07-22T14:08:09.123+08:00",
                  displaySettings.timeFormat,
                ),
              })}
            </p>
            <div className="flex items-start justify-between gap-3 rounded-md bg-muted/35 px-2.5 py-2">
              <div className="min-w-0 space-y-1">
                <Label htmlFor="log-show-elapsed" className="text-xs">
                  {t(WEBUI.logs.showElapsed)}
                </Label>
                <p className="text-[11px] leading-4 text-muted-foreground">
                  {t(WEBUI.logs.showElapsedDescription)}
                </p>
              </div>
              <Switch
                id="log-show-elapsed"
                size="sm"
                checked={displaySettings.showElapsed}
                onCheckedChange={(checked) =>
                  setDisplaySettings((current) => ({
                    ...current,
                    showElapsed: checked,
                  }))
                }
              />
            </div>
          </PopoverContent>
        </Popover>

        <div className="flex-1" />

        {/* Entry count */}
        <span className="text-xs text-muted-foreground shrink-0">
          {t(WEBUI.logs.entryCount, { count: filtered.length })}
          {backlog > 0 && (
            <span className="ml-1 text-amber-400">
              {t(WEBUI.logs.pendingCount, { count: backlog })}
            </span>
          )}
        </span>

        {/* Wrap toggle */}
        <Button
          variant={wrap ? "default" : "outline"}
          size="sm"
          className="h-7 px-2 text-xs"
          onClick={() => setWrap((w) => !w)}
          title={wrap ? t(WEBUI.logs.disableWrap) : t(WEBUI.logs.enableWrap)}
        >
          <WrapText className="size-3 mr-1" />
          {t(WEBUI.logs.wrap)}
        </Button>

        {/* Clear */}
        <Button
          variant="ghost"
          size="sm"
          className="h-7 px-2 text-xs"
          onClick={clearLogs}
        >
          <Trash2 className="size-3 mr-1" />
          {t(WEBUI.common.clear)}
        </Button>

        {/* Pause / Resume */}
        <Button
          variant={paused ? "default" : "outline"}
          size="sm"
          className="h-7 px-2 text-xs"
          onClick={togglePause}
        >
          {paused ? (
            <>
              <Play className="size-3 mr-1" />
              {t(WEBUI.logs.resume, { count: backlog })}
            </>
          ) : (
            <>
              <Pause className="size-3 mr-1" />
              {t(WEBUI.logs.pause)}
            </>
          )}
        </Button>
      </div>

      {/* Log content */}
      <div
        ref={scrollRef}
        className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto overscroll-contain bg-slate-950/65 dark:bg-black/65 backdrop-blur-xl p-2 pb-4 font-mono text-xs leading-relaxed border border-white/10"
      >
        {filtered.length === 0 ? (
          <div className="flex items-center justify-center h-full text-zinc-600">
            {connected
              ? t(WEBUI.logs.waiting)
              : t(WEBUI.logs.connectingBackend)}
          </div>
        ) : (
          filtered.map((entry, index) => (
            <LogLine
              key={`${entry.id}-${entry.elapsed_ms}-${index}`}
              entry={entry}
              wrap={wrap}
              timeFormat={displaySettings.timeFormat}
              showElapsed={displaySettings.showElapsed}
            />
          ))
        )}
      </div>
    </div>
  );
}
