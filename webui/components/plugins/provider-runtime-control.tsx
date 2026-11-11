/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { useEffect } from "react";
import {
  AlertCircleIcon,
  CheckCircle2Icon,
  DatabaseIcon,
  RefreshCwIcon,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Spinner } from "@/components/ui/spinner";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { usePluginAppliedStatus } from "@/hooks/use-plugin-applied";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import { providerHasLiveDependents } from "@/lib/provider-reload";
import { useAppStore } from "@/lib/store";
import type { PluginInstance } from "@/lib/types";
import { cn } from "@/lib/utils";

type ProviderRuntimeControlProps = {
  plugin: PluginInstance;
  mode?: "compact" | "detail";
};

export function ProviderRuntimeControl({
  plugin,
  mode = "compact",
}: ProviderRuntimeControlProps) {
  const { t } = useI18n();
  const appliedStatus = usePluginAppliedStatus(plugin.name);
  const reloadState = useAppStore(
    (state) => state.providerReloads[plugin.name],
  );
  const reloadProvider = useAppStore((state) => state.reloadProvider);
  const clearResult = useAppStore((state) => state.clearProviderReloadResult);
  const isOfflineMode = useAppStore((state) => state.isOfflineMode);
  const runningDependencyGraph = useAppStore(
    (state) => state.runningDependencyGraph,
  );

  const pending = Boolean(reloadState?.pending);
  const outcome = reloadState?.outcome ?? "idle";
  const liveStatus = providerHasLiveDependents(
    plugin.name,
    runningDependencyGraph,
  );
  const skipped = liveStatus === false;
  const unavailable =
    isOfflineMode || appliedStatus === "not-applied" || liveStatus !== true;

  useEffect(() => {
    if (outcome !== "success") return;
    const timeout = window.setTimeout(() => clearResult(plugin.id), 4000);
    return () => window.clearTimeout(timeout);
  }, [clearResult, outcome, plugin.id]);

  if (plugin.type !== "provider") return null;

  const statusLabel = unavailable
    ? skipped
      ? t(WEBUI.plugins.providerReloadSkipped)
      : appliedStatus === "not-applied"
        ? t(WEBUI.plugins.providerReloadNotApplied)
        : t(WEBUI.plugins.providerReloadUnavailable)
    : pending
      ? t(WEBUI.plugins.providerReloadPending)
      : outcome === "success"
        ? t(WEBUI.plugins.providerReloadSuccess)
        : outcome === "error"
          ? t(WEBUI.plugins.providerReloadFailed)
          : t(WEBUI.plugins.providerReloadReady);

  const handleReload = () => {
    void reloadProvider(plugin.id).catch(() => {
      // The store keeps the backend error beside this provider.
    });
  };

  const actionButton = (
    <Button
      variant={mode === "compact" ? "ghost" : "outline"}
      size={mode === "compact" ? "icon-xs" : "sm"}
      className={cn(
        mode === "compact" &&
          "text-muted-foreground hover:bg-muted hover:text-foreground",
        outcome === "success" &&
          "border-primary/30 bg-primary/10 text-primary hover:bg-primary/15 hover:text-primary",
        outcome === "error" &&
          "border-destructive/30 text-destructive hover:bg-destructive/10 hover:text-destructive",
      )}
      aria-label={
        mode === "compact"
          ? outcome === "idle" && !pending
            ? t(WEBUI.plugins.providerReloadAction)
            : statusLabel
          : undefined
      }
      disabled={unavailable || pending}
      onClick={handleReload}
    >
      {pending ? (
        <Spinner data-icon="inline-start" />
      ) : outcome === "success" ? (
        <CheckCircle2Icon data-icon="inline-start" />
      ) : outcome === "error" ? (
        <AlertCircleIcon data-icon="inline-start" />
      ) : (
        <RefreshCwIcon data-icon="inline-start" />
      )}
      {mode === "detail"
        ? pending
          ? t(WEBUI.plugins.providerReloadPending)
          : t(WEBUI.plugins.providerReloadAction)
        : null}
    </Button>
  );

  if (mode === "compact") {
    return (
      <div
        className="flex items-center"
        onClick={(event) => event.stopPropagation()}
        onPointerDown={(event) => event.stopPropagation()}
      >
        <Tooltip>
          <TooltipTrigger asChild>
            <span className="inline-flex">{actionButton}</span>
          </TooltipTrigger>
          <TooltipContent side="bottom">
            {reloadState?.error
              ? `${statusLabel}: ${reloadState.error}`
              : outcome === "idle" && !unavailable
                ? t(WEBUI.plugins.providerReloadAction)
                : statusLabel}
          </TooltipContent>
        </Tooltip>
      </div>
    );
  }

  return (
    <Card
      className={cn(
        "mt-4",
        outcome === "error" && "border-destructive/40 bg-destructive/5",
      )}
    >
      <CardHeader className="flex flex-row items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <CardTitle className="text-sm">
            {t(WEBUI.plugins.providerRuntimeControl)}
          </CardTitle>
          <CardDescription>
            {t(WEBUI.plugins.providerReloadDescription)}
          </CardDescription>
        </div>
        <Badge
          variant={outcome === "error" ? "destructive" : "secondary"}
          aria-live="polite"
        >
          {pending ? (
            <Spinner data-icon="inline-start" />
          ) : outcome === "success" ? (
            <CheckCircle2Icon data-icon="inline-start" />
          ) : outcome === "error" ? (
            <AlertCircleIcon data-icon="inline-start" />
          ) : (
            <DatabaseIcon data-icon="inline-start" />
          )}
          {statusLabel}
        </Badge>
      </CardHeader>
      <CardContent className="flex flex-col gap-2 text-xs text-muted-foreground">
        <p>{t(WEBUI.plugins.providerReloadTopologyHint)}</p>
        {reloadState?.error ? (
          <p className="text-destructive">
            {t(WEBUI.plugins.providerReloadFailed)}: {reloadState.error}
          </p>
        ) : null}
      </CardContent>
      <CardFooter>{actionButton}</CardFooter>
    </Card>
  );
}
