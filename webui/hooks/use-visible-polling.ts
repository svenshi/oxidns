"use client";

import { useEffect } from "react";

type PollingTask = (signal: AbortSignal) => void | Promise<void>;
type PollingSessionKey = string | number | boolean | null | undefined;

/**
 * Runs immediately when enabled or when the session key changes, then keeps
 * one task at a time active while the document is visible. The next run is
 * scheduled only after the previous one settles, so slow requests never
 * accumulate overlapping polling rounds. Set runInBackground for lightweight
 * tasks that should continue when the browser tab is hidden.
 */
export function useVisiblePolling(
  task: PollingTask,
  intervalMs: number,
  enabled = true,
  sessionKey?: PollingSessionKey,
  runInBackground = false,
) {
  useEffect(() => {
    if (!enabled) return;

    let disposed = false;
    let running = false;
    let timer: number | null = null;
    let controller: AbortController | null = null;

    const clearTimer = () => {
      if (timer !== null) {
        window.clearTimeout(timer);
        timer = null;
      }
    };

    const schedule = () => {
      clearTimer();
      if (
        disposed ||
        (!runInBackground && document.visibilityState !== "visible")
      ) {
        return;
      }
      timer = window.setTimeout(() => {
        void run();
      }, intervalMs);
    };

    const run = async () => {
      if (
        disposed ||
        running ||
        (!runInBackground && document.visibilityState !== "visible")
      ) {
        return;
      }
      running = true;
      controller = new AbortController();
      try {
        await task(controller.signal);
      } catch {
        // A later polling round can recover from transient API failures.
      } finally {
        controller = null;
        running = false;
        schedule();
      }
    };

    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        clearTimer();
        void run();
      } else {
        clearTimer();
        controller?.abort();
      }
    };

    if (!runInBackground) {
      document.addEventListener("visibilitychange", handleVisibilityChange);
    }
    void run();

    return () => {
      disposed = true;
      clearTimer();
      controller?.abort();
      if (!runInBackground) {
        document.removeEventListener(
          "visibilitychange",
          handleVisibilityChange,
        );
      }
    };
  }, [enabled, intervalMs, runInBackground, sessionKey, task]);
}
