"use client";

import { useState } from "react";
import {
  Rocket,
  History,
  AlertCircle,
  MoreHorizontal,
  GitCompare,
  Undo2,
  RefreshCw,
  Power,
  RotateCw,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { useAppStore } from "@/lib/store";
import { useAuthStore } from "@/lib/auth-store";
import type { ConfigSnapshot } from "@/lib/config-history";
import { ConfigDiffDialog } from "@/components/config/config-diff-dialog";
import { topLevelConfigChanged } from "@/lib/oxidns-config";

export type SyncState =
  | "in-sync"
  | "not-applied"
  | "applying"
  | "apply-failed"
  | "error";

export interface ConfigSyncStatus {
  state: SyncState;
  label: string;
  tone: "neutral" | "warning" | "destructive";
  head: ConfigSnapshot | undefined;
  /** Snapshot of the config that is currently running on the backend. */
  lastGood: ConfigSnapshot | undefined;
  /**
   * True when the pending change includes top-level fields (runtime, api,
   * log, include, …) that the backend cannot hot-reload. In that case the
   * apply pill must offer a full restart instead of a hot-reload, because
   * hot-reloading would silently leave those fields stale.
   */
  requiresRestart: boolean;
}

// Single source of truth for "is the on-disk config in sync with what's
// running" — shared by the global header control and the editor so the two
// never disagree.
export function useConfigSyncStatus(): ConfigSyncStatus {
  const configHistory = useAppStore((s) => s.configHistory);
  const configError = useAppStore((s) => s.configError);
  const configVersion = useAppStore((s) => s.configVersion);
  const runningVersion = useAppStore((s) => s.runningVersion);
  const isApplying = useAppStore((s) => s.isApplying);

  // Snapshot of the config currently on disk (what 应用 would push live).
  const current = configHistory.find((s) => s.version === configVersion);
  // Snapshot of what the backend is actually running right now. applyStatus
  // is a sticky per-entry flag (an old "applied" never clears), so we resolve
  // the running config by runningVersion — not by find(applied).
  const lastGood =
    configHistory.find((s) => s.version === runningVersion) ??
    configHistory.find((s) => s.applyStatus === "applied");

  // Top-level fields (runtime/api/log/include) are not hot-reloadable. If the
  // pending diff touches any of them, the apply pill must offer a restart.
  const requiresRestart = Boolean(
    current &&
    lastGood &&
    current.content !== lastGood.content &&
    topLevelConfigChanged(current.content, lastGood.content),
  );

  let state: SyncState = "in-sync";
  let tone: ConfigSyncStatus["tone"] = "neutral";
  let label = "配置已同步";

  if (isApplying) {
    state = "applying";
    label = "正在应用配置…";
  } else if (configError) {
    state = "error";
    tone = "destructive";
    label = "配置有错误，无法应用；可在菜单中恢复有效版本";
  } else if (
    configVersion &&
    runningVersion &&
    configVersion === runningVersion
  ) {
    // On-disk config == what's running → nothing to apply.
    state = "in-sync";
  } else if (configVersion && current?.applyStatus === "apply-failed") {
    state = "apply-failed";
    tone = "destructive";
    label = requiresRestart
      ? `重启失败或未完成：${current.applyError ?? "需重启服务才能生效"}`
      : `应用失败：${current.applyError ?? "热重载未成功"}`;
  } else if (configVersion) {
    state = "not-applied";
    tone = "warning";
    label = requiresRestart
      ? "顶层配置（runtime / api / log 等）已变更，需重启服务才能生效"
      : "有未应用的配置改动，点击应用";
  }

  return { state, label, tone, head: current, lastGood, requiresRestart };
}

const PILL_TONE: Record<"warning" | "destructive", string> = {
  warning:
    "border-yellow-500/40 bg-yellow-500/10 text-yellow-700 hover:bg-yellow-500/20 hover:text-yellow-700 dark:text-yellow-300 dark:hover:text-yellow-300",
  destructive:
    "border-destructive/40 bg-destructive/10 text-destructive hover:bg-destructive/20 hover:text-destructive",
};

// One shared, mode-agnostic config-sync control for the global AppHeader.
// In-sync it is near-invisible (a calm history icon + overflow menu); when
// there are pending / failed / invalid changes it becomes a prominent
// amber/red pill so the operator never misses an unapplied change.
export function ConfigSyncControl() {
  const isConnected = useAuthStore((s) => s.isConnected);
  const applyConfig = useAppStore((s) => s.applyConfig);
  const restartApp = useAppStore((s) => s.restartApp);
  const isRestarting = useAppStore((s) => s.isRestarting);
  const isConfigSaving = useAppStore((s) => s.isConfigSaving);
  const restoreSnapshot = useAppStore((s) => s.restoreSnapshot);
  const saveConfig = useAppStore((s) => s.saveConfig);
  const setHistoryOpen = useAppStore((s) => s.setHistoryOpen);
  const configText = useAppStore((s) => s.configText);
  const configVersion = useAppStore((s) => s.configVersion);
  const { state, label, tone, lastGood, requiresRestart } =
    useConfigSyncStatus();

  const [diffOpen, setDiffOpen] = useState(false);
  const [restartConfirmOpen, setRestartConfirmOpen] = useState(false);

  if (!isConnected) return null;

  const hasLoadedConfig = Boolean(configVersion);
  const pendingRestartOnlyChange =
    requiresRestart && (state === "not-applied" || state === "apply-failed");

  const handleApply = async () => {
    if (!hasLoadedConfig) return;
    if (pendingRestartOnlyChange) {
      setRestartConfirmOpen(true);
      return;
    }
    try {
      await applyConfig();
    } catch {
      // Surfaced via the snapshot status (red pill + label).
    }
  };

  const handleRestart = async () => {
    if (!hasLoadedConfig) return;
    try {
      await restartApp();
    } catch {
      // Restart failures are annotated on the current config snapshot, so the
      // header pill stays red and offers another restart attempt.
    }
  };

  // "Discard pending change, go back to the running config." Loads the
  // running config into the buffer AND writes it to disk, so disk == running
  // and the sync status returns to in-sync (pill clears) — without needing a
  // reload, since the running config is already what's live.
  const handleRevertToRunning = async () => {
    if (!lastGood) return;
    restoreSnapshot(lastGood.id);
    try {
      await saveConfig();
    } catch {
      // Validation/save failure is surfaced via configError (error pill).
    }
  };

  const canDiff = Boolean(lastGood && lastGood.content !== configText);
  const canRestore = Boolean(lastGood && lastGood.version !== configVersion);

  const pillClass = tone === "neutral" ? "" : PILL_TONE[tone];

  const primary =
    state === "in-sync" ? (
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            size="icon-sm"
            className="rounded-md"
            onClick={() => setHistoryOpen(true)}
          >
            <History className="h-4 w-4" />
            <span className="sr-only">配置历史</span>
          </Button>
        </TooltipTrigger>
        <TooltipContent>配置历史（已同步）</TooltipContent>
      </Tooltip>
    ) : state === "applying" ? (
      <Button
        variant="outline"
        size="sm"
        className="h-7 gap-1.5 rounded-md px-2.5"
        disabled
      >
        <Spinner className="h-3.5 w-3.5" />
        {isRestarting ? "重启中" : "应用中"}
      </Button>
    ) : isRestarting ? (
      <Button
        variant="outline"
        size="sm"
        className="h-7 gap-1.5 rounded-md px-2.5"
        disabled
      >
        <Spinner className="h-3.5 w-3.5" />
        重启中
      </Button>
    ) : (
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="outline"
            size="sm"
            className={`h-7 gap-1.5 rounded-md px-2.5 ${pillClass}`}
            onClick={() => {
              if (pendingRestartOnlyChange) {
                setRestartConfirmOpen(true);
              } else {
                void handleApply();
              }
            }}
            disabled={state === "error" || isConfigSaving}
          >
            {state === "not-applied" || state === "apply-failed" ? (
              pendingRestartOnlyChange ? (
                <RotateCw className="h-3.5 w-3.5" />
              ) : (
                <Rocket className="h-3.5 w-3.5" />
              )
            ) : (
              <AlertCircle className="h-3.5 w-3.5" />
            )}
            {state === "not-applied"
              ? requiresRestart
                ? "需要重启"
                : "应用更改"
              : state === "apply-failed"
                ? pendingRestartOnlyChange
                  ? "需要重启"
                  : "应用失败·重试"
                : "配置有误"}
          </Button>
        </TooltipTrigger>
        <TooltipContent>{label}</TooltipContent>
      </Tooltip>
    );

  return (
    <>
      <div className="flex items-center gap-1">
        {primary}
        <DropdownMenu>
          <Tooltip>
            <TooltipTrigger asChild>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="icon-sm" className="rounded-md">
                  <MoreHorizontal className="h-4 w-4" />
                  <span className="sr-only">配置操作</span>
                </Button>
              </DropdownMenuTrigger>
            </TooltipTrigger>
            <TooltipContent>配置操作</TooltipContent>
          </Tooltip>
          <DropdownMenuContent align="end" className="w-44">
            <DropdownMenuItem
              disabled={!canDiff}
              onClick={() => setDiffOpen(true)}
            >
              <GitCompare className="h-4 w-4" />
              查看差异
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setHistoryOpen(true)}>
              <History className="h-4 w-4" />
              配置历史
            </DropdownMenuItem>
            {canRestore && lastGood && (
              <DropdownMenuItem onClick={handleRevertToRunning}>
                <Undo2 className="h-4 w-4" />
                放弃改动·恢复运行版
              </DropdownMenuItem>
            )}
            <DropdownMenuSeparator />
            <DropdownMenuItem
              disabled={
                state === "applying" ||
                state === "error" ||
                isRestarting ||
                isConfigSaving ||
                pendingRestartOnlyChange ||
                !hasLoadedConfig
              }
              onClick={handleApply}
            >
              <RefreshCw className="h-4 w-4" />
              重载当前配置
            </DropdownMenuItem>
            <DropdownMenuItem
              disabled={
                state === "applying" ||
                state === "error" ||
                isRestarting ||
                isConfigSaving ||
                !hasLoadedConfig
              }
              onClick={(event) => {
                // Prevent the menu's default focus-restore so the AlertDialog
                // can take focus cleanly after the menu closes.
                event.preventDefault();
                setRestartConfirmOpen(true);
              }}
            >
              <Power className="h-4 w-4" />
              重启服务
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      {lastGood && (
        <ConfigDiffDialog
          open={diffOpen}
          onOpenChange={setDiffOpen}
          original={lastGood.content}
          modified={configText}
          originalTitle="正在运行"
          modifiedTitle="待应用（当前配置）"
        />
      )}

      <AlertDialog
        open={restartConfirmOpen}
        onOpenChange={setRestartConfirmOpen}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>重启 OxiDNS 服务？</AlertDialogTitle>
            <AlertDialogDescription>
              将以新进程替换正在运行的服务。期间 DNS
              解析会短暂中断，所有内存中的状态（如缓存）将被清空。配置会先保存到磁盘再触发重启。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction onClick={handleRestart}>
              确认重启
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
