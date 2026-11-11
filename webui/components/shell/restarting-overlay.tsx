/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { useEffect } from "react";
import { Loader2, Check, Power } from "lucide-react";
import { useAppStore, type RestartPhase } from "@/lib/store";
import { cn } from "@/lib/utils";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

const PHASE_ORDER: RestartPhase[] = [
  "saving",
  "requesting",
  "waiting_down",
  "waiting_up",
  "reloading",
];

// Full-screen modal that blocks every interaction while the backend is being
// restarted. Sits above sheets/dialogs and traps focus so the user cannot
// click into stale UI state while DNS is briefly unavailable.
export function RestartingOverlay() {
  const { t } = useI18n();
  const isRestarting = useAppStore((s) => s.isRestarting);
  const restartPhase = useAppStore((s) => s.restartPhase);

  // Lock body scroll while the overlay is active so background lists can't be
  // scrolled behind it. Restored on unmount / restart finish.
  useEffect(() => {
    if (!isRestarting) return;
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = previous;
    };
  }, [isRestarting]);

  if (!isRestarting) return null;

  const currentIndex = restartPhase ? PHASE_ORDER.indexOf(restartPhase) : -1;

  return (
    <div
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="restart-overlay-title"
      aria-describedby="restart-overlay-description"
      // Block every pointer/keyboard interaction with the underlying app while
      // restart is in flight. The overlay itself never closes from a click.
      onClickCapture={(e) => e.stopPropagation()}
      onKeyDownCapture={(e) => {
        // Don't let Escape close any underlying dialogs / sheets.
        if (e.key === "Escape") e.stopPropagation();
      }}
      className="fixed inset-0 z-[100] flex items-center justify-center bg-background/80 backdrop-blur-sm"
    >
      <div className="mx-4 w-full max-w-md rounded-xl border bg-card p-6 shadow-2xl">
        <div className="flex items-center gap-3">
          <div className="flex size-10 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
            <Power className="h-5 w-5" />
          </div>
          <div className="min-w-0">
            <h2
              id="restart-overlay-title"
              className="text-sm font-semibold leading-tight"
            >
              {t(WEBUI.restartOverlay.title)}
            </h2>
            <p
              id="restart-overlay-description"
              className="mt-1 text-xs text-muted-foreground"
            >
              {t(WEBUI.restartOverlay.description)}
            </p>
          </div>
        </div>

        <ul className="mt-5 space-y-2">
          {PHASE_ORDER.map((phase, index) => {
            const isDone = currentIndex > index;
            const isActive = currentIndex === index;
            return (
              <li
                key={phase}
                className={cn(
                  "flex items-center gap-2.5 text-xs",
                  isActive
                    ? "text-foreground"
                    : isDone
                      ? "text-muted-foreground"
                      : "text-muted-foreground/60",
                )}
              >
                <span className="flex size-4 shrink-0 items-center justify-center">
                  {isDone ? (
                    <Check className="h-3.5 w-3.5 text-primary" />
                  ) : isActive ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin text-primary" />
                  ) : (
                    <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground/40" />
                  )}
                </span>
                <span className={cn(isActive && "font-medium")}>
                  {restartPhaseLabel(phase, t)}
                </span>
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}

function restartPhaseLabel(
  phase: RestartPhase,
  t: ReturnType<typeof useI18n>["t"],
) {
  switch (phase) {
    case "saving":
      return t(WEBUI.restartOverlay.saving);
    case "requesting":
      return t(WEBUI.restartOverlay.requesting);
    case "waiting_down":
      return t(WEBUI.restartOverlay.waitingDown);
    case "waiting_up":
      return t(WEBUI.restartOverlay.waitingUp);
    case "reloading":
      return t(WEBUI.restartOverlay.reloading);
  }
}
