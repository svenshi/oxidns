/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { useState, type MouseEvent } from "react";
import {
  RotateCcwIcon,
  SlidersHorizontalIcon,
  ToggleLeftIcon,
  ToggleRightIcon,
} from "lucide-react";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Spinner } from "@/components/ui/spinner";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import {
  planMatcherModeChange,
  type FixedMatcherRuntimeMode,
  type MatcherRuntimeMode,
} from "@/lib/matcher-control";
import { useAppStore } from "@/lib/store";
import type { PluginInstance } from "@/lib/types";
import { cn } from "@/lib/utils";

type MatcherRuntimeControlProps = {
  plugin: PluginInstance;
  mode?: "compact" | "detail";
};

export function MatcherRuntimeControl({
  plugin,
  mode = "compact",
}: MatcherRuntimeControlProps) {
  const { t } = useI18n();
  const [confirmMode, setConfirmMode] =
    useState<FixedMatcherRuntimeMode | null>(null);
  const control = useAppStore((state) => state.matcherControls[plugin.name]);
  const setMatcherMode = useAppStore((state) => state.setMatcherMode);

  if (plugin.type !== "matcher") return null;

  const ready = control?.availability === "ready" && control.mode !== null;
  const pending = Boolean(control?.pending);
  const currentMode = ready ? control.mode : null;
  const alwaysFalse = currentMode === "always_false";
  const alwaysTrue = currentMode === "always_true";
  const fixed = alwaysFalse || alwaysTrue;
  const loading = control?.availability === "loading";
  const unavailable = !ready && !loading;
  const positiveReference = `$${plugin.name}`;
  const negativeReference = `!$${plugin.name}`;

  const statusLabel = ready
    ? alwaysFalse
      ? t(WEBUI.plugins.matcherAlwaysFalse)
      : alwaysTrue
        ? t(WEBUI.plugins.matcherAlwaysTrue)
        : t(WEBUI.plugins.matcherRuntimeNormal)
    : loading
      ? t(WEBUI.plugins.matcherControlLoading)
      : t(WEBUI.plugins.matcherControlUnavailable);

  const statusDescription = ready
    ? alwaysFalse
      ? t(WEBUI.plugins.matcherAlwaysFalseDescription)
      : alwaysTrue
        ? t(WEBUI.plugins.matcherAlwaysTrueDescription)
        : t(WEBUI.plugins.matcherRuntimeNormalDescription)
    : loading
      ? t(WEBUI.plugins.matcherControlLoading)
      : t(WEBUI.plugins.matcherControlUnavailable);

  const applyFixedMode = async (event: MouseEvent<HTMLButtonElement>) => {
    event.preventDefault();
    if (!confirmMode) return;
    try {
      await setMatcherMode(plugin.id, confirmMode);
      setConfirmMode(null);
    } catch {
      // The store keeps the backend error beside this matcher.
    }
  };

  const requestModeChange = (nextMode: MatcherRuntimeMode) => {
    const plan = planMatcherModeChange(nextMode);
    if (plan.kind === "confirm") {
      setConfirmMode(plan.mode);
      return;
    }
    void setMatcherMode(plugin.id, plan.mode).catch(() => {
      // The store keeps the backend error beside this matcher.
    });
  };

  const modeIcon = loading ? (
    <Spinner data-icon="inline-start" />
  ) : alwaysFalse ? (
    <ToggleLeftIcon data-icon="inline-start" />
  ) : alwaysTrue ? (
    <ToggleRightIcon data-icon="inline-start" />
  ) : (
    <SlidersHorizontalIcon data-icon="inline-start" />
  );

  const actionMenu = (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon-xs"
          className="text-muted-foreground hover:bg-muted hover:text-foreground"
          aria-label={t(WEBUI.plugins.matcherModeActions)}
          disabled={!ready || pending}
        >
          {modeIcon}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-44">
        <DropdownMenuLabel>{statusLabel}</DropdownMenuLabel>
        <DropdownMenuSeparator />
        {fixed ? (
          <DropdownMenuItem onSelect={() => requestModeChange("normal")}>
            <RotateCcwIcon />
            {t(WEBUI.plugins.matcherRestoreAction)}
          </DropdownMenuItem>
        ) : null}
        {!alwaysFalse ? (
          <DropdownMenuItem onSelect={() => requestModeChange("always_false")}>
            <ToggleLeftIcon />
            {t(WEBUI.plugins.matcherAlwaysFalseAction)}
          </DropdownMenuItem>
        ) : null}
        {!alwaysTrue ? (
          <DropdownMenuItem
            variant="destructive"
            onSelect={() => requestModeChange("always_true")}
          >
            <ToggleRightIcon />
            {t(WEBUI.plugins.matcherAlwaysTrueAction)}
          </DropdownMenuItem>
        ) : null}
      </DropdownMenuContent>
    </DropdownMenu>
  );

  const confirmation = (
    <AlertDialog
      open={confirmMode !== null}
      onOpenChange={(open) => {
        if (!open && !pending) setConfirmMode(null);
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogMedia
            className={cn(
              confirmMode === "always_true"
                ? "bg-destructive/10 text-destructive"
                : "bg-warning/15 text-warning-foreground",
            )}
          >
            {confirmMode === "always_true" ? (
              <ToggleRightIcon />
            ) : (
              <ToggleLeftIcon />
            )}
          </AlertDialogMedia>
          <AlertDialogTitle>
            {t(
              confirmMode === "always_true"
                ? WEBUI.plugins.matcherAlwaysTrueConfirmTitle
                : WEBUI.plugins.matcherAlwaysFalseConfirmTitle,
              { tag: plugin.name },
            )}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {t(
              confirmMode === "always_true"
                ? WEBUI.plugins.matcherAlwaysTrueConfirmDescription
                : WEBUI.plugins.matcherAlwaysFalseConfirmDescription,
            )}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <div className="flex flex-col gap-2 text-sm text-muted-foreground">
          <p>
            {t(
              confirmMode === "always_true"
                ? WEBUI.plugins.matcherAlwaysTrueImpact
                : WEBUI.plugins.matcherAlwaysFalseImpact,
              {
                positive: positiveReference,
                negative: negativeReference,
              },
            )}
          </p>
          <p>{t(WEBUI.plugins.matcherModeResetHint)}</p>
          {control?.error ? (
            <p className="text-destructive">
              {t(WEBUI.plugins.matcherControlFailed)}: {control.error}
            </p>
          ) : null}
        </div>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={pending}>
            {t(WEBUI.common.cancel)}
          </AlertDialogCancel>
          <AlertDialogAction
            variant={confirmMode === "always_true" ? "destructive" : "warning"}
            disabled={pending || confirmMode === null}
            onClick={applyFixedMode}
          >
            {pending ? <Spinner data-icon="inline-start" /> : null}
            {t(
              confirmMode === "always_true"
                ? WEBUI.plugins.matcherAlwaysTrueConfirmAction
                : WEBUI.plugins.matcherAlwaysFalseConfirmAction,
            )}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );

  const detailActions = (
    <div className="flex flex-wrap items-center gap-2">
      {fixed ? (
        <Button
          variant="outline"
          size="sm"
          disabled={!ready || pending}
          onClick={() => requestModeChange("normal")}
        >
          {pending ? <Spinner data-icon="inline-start" /> : <RotateCcwIcon />}
          {t(WEBUI.plugins.matcherRestoreAction)}
        </Button>
      ) : null}
      {!alwaysFalse ? (
        <Button
          variant="warning"
          size="sm"
          disabled={!ready || pending}
          onClick={() => requestModeChange("always_false")}
        >
          <ToggleLeftIcon />
          {t(WEBUI.plugins.matcherAlwaysFalseAction)}
        </Button>
      ) : null}
      {!alwaysTrue ? (
        <Button
          variant="destructive"
          size="sm"
          disabled={!ready || pending}
          onClick={() => requestModeChange("always_true")}
        >
          <ToggleRightIcon />
          {t(WEBUI.plugins.matcherAlwaysTrueAction)}
        </Button>
      ) : null}
    </div>
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
            <span className="inline-flex">{actionMenu}</span>
          </TooltipTrigger>
          <TooltipContent side="bottom">
            {unavailable && control?.error
              ? `${statusLabel}: ${control.error}`
              : statusLabel}
          </TooltipContent>
        </Tooltip>
        {confirmation}
      </div>
    );
  }

  return (
    <Card
      className={cn(
        "mt-4",
        alwaysFalse && "border-warning/40 bg-warning/5",
        alwaysTrue && "border-destructive/40 bg-destructive/5",
      )}
    >
      <CardHeader className="flex flex-row items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <CardTitle className="text-sm">
            {t(WEBUI.plugins.matcherRuntimeControl)}
          </CardTitle>
          <CardDescription>{statusDescription}</CardDescription>
        </div>
        <Badge
          variant={
            alwaysTrue
              ? "destructive"
              : alwaysFalse
                ? "warning"
                : ready
                  ? "secondary"
                  : "outline"
          }
        >
          {modeIcon}
          {statusLabel}
        </Badge>
      </CardHeader>
      {fixed || control?.error ? (
        <CardContent className="flex flex-col gap-2 text-xs text-muted-foreground">
          {fixed ? (
            <>
              <p>
                {t(
                  alwaysTrue
                    ? WEBUI.plugins.matcherAlwaysTrueImpact
                    : WEBUI.plugins.matcherAlwaysFalseImpact,
                  {
                    positive: positiveReference,
                    negative: negativeReference,
                  },
                )}
              </p>
              <p>{t(WEBUI.plugins.matcherModeResetHint)}</p>
            </>
          ) : null}
          {control?.error ? (
            <p className="text-destructive">
              {t(WEBUI.plugins.matcherControlFailed)}: {control.error}
            </p>
          ) : null}
        </CardContent>
      ) : null}
      <CardFooter>{detailActions}</CardFooter>
      {confirmation}
    </Card>
  );
}
