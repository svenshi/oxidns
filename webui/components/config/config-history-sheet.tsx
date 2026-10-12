/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { useState } from "react";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { GitCompare, Trash2, RotateCcw, History } from "lucide-react";
import { useAppStore } from "@/lib/store";
import type { ConfigSnapshot } from "@/lib/config-history";
import { ConfigDiffDialog } from "@/components/config/config-diff-dialog";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

interface ConfigHistorySheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

function formatSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KB`;
}

export function ConfigHistorySheet({
  open,
  onOpenChange,
}: ConfigHistorySheetProps) {
  const { t, formatDateTime } = useI18n();
  const configHistory = useAppStore((s) => s.configHistory);
  const configText = useAppStore((s) => s.configText);
  const configVersion = useAppStore((s) => s.configVersion);
  const runningVersion = useAppStore((s) => s.runningVersion);
  const rollbackToSnapshot = useAppStore((s) => s.rollbackToSnapshot);
  const deleteConfigSnapshot = useAppStore((s) => s.deleteConfigSnapshot);
  const clearConfigHistory = useAppStore((s) => s.clearConfigHistory);

  const [diffEntry, setDiffEntry] = useState<ConfigSnapshot | null>(null);

  const handleRollback = async (id: string) => {
    onOpenChange(false);
    try {
      await rollbackToSnapshot(id);
    } catch {
      // Surfaced via the header sync pill (red) / editor error badge.
    }
  };

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent className="flex w-full flex-col gap-0 bg-background p-0 sm:max-w-md">
        <SheetHeader className="border-b px-5 py-4">
          <SheetTitle className="flex items-center gap-2 text-base">
            <History className="h-4 w-4 text-muted-foreground" />
            {t(WEBUI.configEditor.historyTitle)}
          </SheetTitle>
          <SheetDescription>
            {t(WEBUI.configEditor.historyDesc)}
          </SheetDescription>
        </SheetHeader>

        <ScrollArea className="min-h-0 flex-1">
          <div className="space-y-2 p-4">
            {configHistory.length === 0 && (
              <p className="py-12 text-center text-sm text-muted-foreground">
                {t(WEBUI.configEditor.noHistory)}
              </p>
            )}
            {configHistory.map((entry) => {
              const isRunning = entry.version === runningVersion;
              const isPending = !isRunning && entry.version === configVersion;
              return (
                <div
                  key={entry.id}
                  className="rounded-lg border bg-card/40 p-3"
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0 space-y-1">
                      <div className="flex flex-wrap items-center gap-1.5">
                        <span className="text-sm font-medium">
                          {formatDateTime(entry.createdAt, {
                            year: "numeric",
                            month: "2-digit",
                            day: "2-digit",
                            hour: "2-digit",
                            minute: "2-digit",
                            second: "2-digit",
                          })}
                        </span>
                        {isRunning && (
                          <Badge
                            variant="outline"
                            className="border-primary/30 bg-primary/10 text-primary"
                          >
                            {t(WEBUI.configEditor.runningBadge)}
                          </Badge>
                        )}
                        {isPending && (
                          <Badge
                            variant="outline"
                            className="border-yellow-500/30 bg-yellow-500/10 text-yellow-600 dark:text-yellow-400"
                          >
                            {t(WEBUI.configEditor.pendingBadge)}
                          </Badge>
                        )}
                      </div>
                      <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                        <code className="rounded bg-muted px-1 py-0.5 font-mono">
                          {entry.version.slice(0, 8)}
                        </code>
                        <Badge variant="outline">
                          {entry.source === "server"
                            ? t(WEBUI.configEditor.baselineSource)
                            : t(WEBUI.configEditor.savedSource)}
                        </Badge>
                        <span>
                          {t(WEBUI.configEditor.pluginCount, {
                            count: entry.pluginCount,
                          })}
                        </span>
                        <span>{formatSize(entry.size)}</span>
                      </div>
                      {entry.applyError && (
                        <p className="text-xs text-destructive">
                          {t(WEBUI.configEditor.lastApplyFailed, {
                            error: entry.applyError,
                          })}
                        </p>
                      )}
                    </div>
                  </div>
                  <div className="mt-2.5 flex items-center gap-1.5">
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-7 px-2 text-xs"
                      onClick={() => setDiffEntry(entry)}
                    >
                      <GitCompare className="mr-1 h-3.5 w-3.5" />
                      {t(WEBUI.configEditor.diff)}
                    </Button>
                    <AlertDialog>
                      <AlertDialogTrigger asChild>
                        <Button
                          variant="outline"
                          size="sm"
                          className="h-7 px-2 text-xs"
                          disabled={isRunning}
                        >
                          <RotateCcw className="mr-1 h-3.5 w-3.5" />
                          {t(WEBUI.configEditor.rollbackVersion)}
                        </Button>
                      </AlertDialogTrigger>
                      <AlertDialogContent>
                        <AlertDialogHeader>
                          <AlertDialogTitle>
                            {t(WEBUI.configEditor.rollbackTitle)}
                          </AlertDialogTitle>
                          <AlertDialogDescription>
                            {t(WEBUI.configEditor.rollbackDesc)}
                          </AlertDialogDescription>
                        </AlertDialogHeader>
                        <AlertDialogFooter>
                          <AlertDialogCancel>
                            {t(WEBUI.common.cancel)}
                          </AlertDialogCancel>
                          <AlertDialogAction
                            onClick={() => handleRollback(entry.id)}
                          >
                            {t(WEBUI.configEditor.rollbackAndApply)}
                          </AlertDialogAction>
                        </AlertDialogFooter>
                      </AlertDialogContent>
                    </AlertDialog>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="ml-auto h-7 px-2 text-xs text-muted-foreground hover:text-destructive"
                      onClick={() => deleteConfigSnapshot(entry.id)}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                      <span className="sr-only">
                        {t(WEBUI.configEditor.deleteSnapshot)}
                      </span>
                    </Button>
                  </div>
                </div>
              );
            })}
          </div>
        </ScrollArea>

        {configHistory.length > 0 && (
          <div className="border-t p-4">
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button
                  variant="outline"
                  size="sm"
                  className="w-full text-muted-foreground hover:text-destructive"
                >
                  <Trash2 className="mr-1.5 h-4 w-4" />
                  {t(WEBUI.configEditor.clearHistory)}
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>
                    {t(WEBUI.configEditor.clearHistoryTitle)}
                  </AlertDialogTitle>
                  <AlertDialogDescription>
                    {t(WEBUI.configEditor.clearHistoryDesc)}
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>
                    {t(WEBUI.common.cancel)}
                  </AlertDialogCancel>
                  <AlertDialogAction onClick={() => clearConfigHistory()}>
                    {t(WEBUI.common.clear)}
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          </div>
        )}
      </SheetContent>

      {diffEntry && (
        <ConfigDiffDialog
          open={Boolean(diffEntry)}
          onOpenChange={(o) => {
            if (!o) setDiffEntry(null);
          }}
          original={diffEntry.content}
          modified={configText}
          originalTitle={t(WEBUI.configEditor.snapshotTitle, {
            version: diffEntry.version.slice(0, 8),
          })}
          modifiedTitle={t(WEBUI.configEditor.currentEditor)}
        />
      )}
    </Sheet>
  );
}
