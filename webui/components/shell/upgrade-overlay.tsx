"use client";

import { useEffect } from "react";
import { ArrowUpCircle, Check, Loader2 } from "lucide-react";
import {
  useUpdateStore,
  type UpgradeApplyPhase,
} from "@/lib/update-store";
import { cn } from "@/lib/utils";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

const PHASE_ORDER: UpgradeApplyPhase[] = [
  "requesting",
  "applying",
  "waiting_up",
  "verifying",
  "completed",
];

export function UpgradeOverlay() {
  const { t } = useI18n();
  const isApplying = useUpdateStore((s) => s.isApplying);
  const applyPhase = useUpdateStore((s) => s.applyPhase);

  useEffect(() => {
    if (!isApplying) return;
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = previous;
    };
  }, [isApplying]);

  if (!isApplying && applyPhase !== "completed") return null;

  const currentIndex = applyPhase ? PHASE_ORDER.indexOf(applyPhase) : -1;

  return (
    <div
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="upgrade-overlay-title"
      aria-describedby="upgrade-overlay-description"
      onClickCapture={(e) => e.stopPropagation()}
      onKeyDownCapture={(e) => {
        if (e.key === "Escape") e.stopPropagation();
      }}
      className="fixed inset-0 z-[110] flex items-center justify-center bg-background/80 backdrop-blur-sm"
    >
      <div className="mx-4 w-full max-w-md rounded-xl border bg-card p-6 shadow-2xl">
        <div className="flex items-center gap-3">
          <div className="flex size-10 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
            <ArrowUpCircle className="h-5 w-5" />
          </div>
          <div className="min-w-0">
            <h2
              id="upgrade-overlay-title"
              className="text-sm font-semibold leading-tight"
            >
              {t(WEBUI.settings.upgradeProgressTitle)}
            </h2>
            <p
              id="upgrade-overlay-description"
              className="mt-1 text-xs text-muted-foreground"
            >
              {t(WEBUI.settings.upgradeProgressDesc)}
            </p>
          </div>
        </div>

        <ul className="mt-5 flex flex-col gap-2">
          {PHASE_ORDER.map((phase, index) => {
            const isDone = currentIndex > index;
            const isActive = currentIndex === index;
            const isCompletedActive = isActive && phase === "completed";
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
                  {isDone || isCompletedActive ? (
                    <Check className="h-3.5 w-3.5 text-primary" />
                  ) : isActive ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin text-primary" />
                  ) : (
                    <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground/40" />
                  )}
                </span>
                <span className={cn(isActive && "font-medium")}>
                  {upgradePhaseLabel(phase, t)}
                </span>
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}

function upgradePhaseLabel(
  phase: UpgradeApplyPhase,
  t: ReturnType<typeof useI18n>["t"],
) {
  switch (phase) {
    case "requesting":
      return t(WEBUI.settings.upgradePhaseRequesting);
    case "applying":
      return t(WEBUI.settings.upgradePhaseApplying);
    case "waiting_up":
      return t(WEBUI.settings.upgradePhaseWaitingUp);
    case "verifying":
      return t(WEBUI.settings.upgradePhaseVerifying);
    case "completed":
      return t(WEBUI.settings.upgradePhaseCompleted);
  }
}
