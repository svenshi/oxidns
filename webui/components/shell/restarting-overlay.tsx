"use client";

import { useEffect } from "react";
import { Loader2, Check, Power } from "lucide-react";
import { useAppStore, type RestartPhase } from "@/lib/store";
import { cn } from "@/lib/utils";

const PHASE_LABELS: Record<RestartPhase, string> = {
  saving: "保存配置到磁盘",
  requesting: "发起重启请求",
  waiting_down: "等待旧进程退出",
  waiting_up: "等待新进程就绪",
  reloading: "重新加载配置",
};

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
              正在重启 OxiDNS 服务
            </h2>
            <p
              id="restart-overlay-description"
              className="mt-1 text-xs text-muted-foreground"
            >
              DNS 解析会短暂中断，完成后页面会自动恢复，请勿刷新或关闭。
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
                  {PHASE_LABELS[phase]}
                </span>
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}
