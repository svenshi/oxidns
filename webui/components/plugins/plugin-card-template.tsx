"use client";

import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Pin, PinOff } from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useAppStore } from "@/lib/store";
import { selectCardMetrics } from "@/lib/metrics";
import { isPluginKindSupported } from "@/lib/build-capabilities";
import { cn } from "@/lib/utils";
import { WEBUI } from "@/lib/i18n";
import { pluginTypeLabel } from "@/lib/i18n/plugin-defined";
import { useI18n } from "@/lib/i18n/provider";
import type { PluginCardTemplateProps } from "./types";
import { pluginTypeColors, pluginTypeIconText } from "./display";
import { getPluginCatalogItem, renderPluginKindIcon } from "./catalog";
import { PluginDeleteButton } from "./plugin-delete-button";
import { MatcherRuntimeControl } from "./matcher-runtime-control";
import { ProviderRuntimeControl } from "./provider-runtime-control";
import {
  PluginCardItemGrid,
  PluginCardItemSurface,
} from "./plugin-card-item-grid";

const MAX_CARD_METRICS = 6;

export function PluginCardTemplate({
  plugin,
  compact = false,
  icon,
  primaryMetric,
  children,
}: PluginCardTemplateProps) {
  const { locale, t } = useI18n();
  const { setSelectedPlugin, setDetailOpen, togglePluginPin } = useAppStore();
  const series = useAppStore((s) => s.pluginMetrics[plugin.name]);
  const matcherControl = useAppStore((s) =>
    plugin.type === "matcher" ? s.matcherControls[plugin.name] : undefined,
  );
  const buildInfo = useAppStore((s) => s.buildInfo);
  const cardMetrics = selectCardMetrics(
    series,
    plugin.pluginKind,
    MAX_CARD_METRICS,
    locale,
  );
  const showFallbackContent = cardMetrics.length === 0 && Boolean(children);
  const definition = getPluginCatalogItem(plugin.pluginKind, locale);
  const supported = isPluginKindSupported(
    buildInfo,
    plugin.type,
    plugin.pluginKind,
  );
  const matcherAlwaysFalse =
    matcherControl?.availability === "ready" &&
    matcherControl.mode === "always_false";
  const matcherAlwaysTrue =
    matcherControl?.availability === "ready" &&
    matcherControl.mode === "always_true";
  const resolvedIcon =
    icon ??
    (definition
      ? renderPluginKindIcon(definition.icon, {
          className: "size-4",
        })
      : null);

  const handleClick = () => {
    setSelectedPlugin(plugin);
    setDetailOpen(true);
  };

  return (
    <Card
      className={cn(
        "group relative flex h-full min-h-[10.75rem] cursor-pointer flex-col gap-0 overflow-hidden py-0 transition-[border-color,box-shadow,transform] duration-200 hover:-translate-y-px hover:border-primary/40 hover:shadow-md",
        plugin.pinned && "border-primary/30",
        matcherAlwaysFalse &&
          "border-warning/40 bg-warning/5 hover:border-warning/60",
        matcherAlwaysTrue &&
          "border-destructive/40 bg-destructive/5 hover:border-destructive/60",
        !supported && "border-dashed opacity-70",
      )}
      aria-disabled={!supported}
      onClick={handleClick}
    >
      <CardHeader className="flex min-h-[4.75rem] flex-row items-start justify-between gap-3 px-3.5 pb-2.5 pt-3">
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-2">
            <span
              className={cn(
                "flex size-5 shrink-0 items-center justify-center [&>svg]:size-[1.125rem]",
                pluginTypeIconText[plugin.type],
              )}
            >
              {resolvedIcon}
            </span>
            <div className="flex min-w-0 items-center gap-1.5">
              <span
                className="min-w-0 truncate font-mono text-sm font-semibold leading-5 tracking-[-0.01em]"
                title={plugin.name}
              >
                {plugin.name}
              </span>
              {plugin.pinned && (
                <Pin className="size-3 shrink-0 fill-primary/15 text-primary" />
              )}
            </div>
          </div>
          <div className="mt-1.5 flex min-w-0 items-center gap-1.5 overflow-hidden">
            <Badge
              variant="outline"
              className={cn(
                "h-5 max-w-[45%] rounded-md px-1.5 py-0 text-xs leading-none font-medium",
                pluginTypeColors[plugin.type],
              )}
            >
              <span className="truncate">
                {pluginTypeLabel(plugin.type, locale)}
              </span>
            </Badge>
            <span
              aria-hidden="true"
              className="size-0.5 shrink-0 rounded-full bg-muted-foreground/50"
            />
            <span
              className="min-w-0 truncate text-xs leading-none text-muted-foreground"
              title={definition?.name ?? plugin.pluginKind}
            >
              {definition?.name ?? plugin.pluginKind}
            </span>
            {!supported && (
              <Badge
                variant="outline"
                className="h-5 rounded-md px-1.5 py-0 text-xs leading-none"
              >
                {t(WEBUI.common.notCompiled)}
              </Badge>
            )}
          </div>
          {definition?.description && !compact && !children && (
            <p className="mt-2 line-clamp-2 text-xs leading-4 text-muted-foreground">
              {definition.description}
            </p>
          )}
        </div>
        <div className="flex shrink-0 items-start gap-0.5 rounded-lg p-0.5 transition-colors group-hover:bg-muted/45">
          {primaryMetric && (
            <div className="mr-0.5 rounded-md bg-muted/45 px-2 py-1 text-right ring-1 ring-inset ring-border/35">
              <div className="font-mono text-lg leading-none font-semibold tabular-nums">
                {primaryMetric.value}
              </div>
              <div className="mt-1 text-xs leading-none text-muted-foreground">
                {primaryMetric.label}
              </div>
            </div>
          )}
          {plugin.type === "matcher" ? (
            <MatcherRuntimeControl plugin={plugin} />
          ) : null}
          {plugin.type === "provider" ? (
            <ProviderRuntimeControl plugin={plugin} />
          ) : null}
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className={cn(
                  "size-6 shrink-0 rounded-md transition-opacity focus-visible:opacity-100",
                  plugin.pinned
                    ? "text-primary opacity-100"
                    : "opacity-0 group-hover:opacity-100",
                )}
                onClick={(e) => {
                  e.stopPropagation();
                  togglePluginPin(plugin.id);
                }}
              >
                {plugin.pinned ? (
                  <PinOff className="size-3.5" />
                ) : (
                  <Pin className="size-3.5" />
                )}
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">
              {plugin.pinned
                ? t(WEBUI.plugins.unpin)
                : t(WEBUI.plugins.pinDashboard)}
            </TooltipContent>
          </Tooltip>
          <PluginDeleteButton
            plugin={plugin}
            className="size-6 shrink-0 rounded-md opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 hover:text-destructive"
            size="icon-xs"
          />
        </div>
      </CardHeader>
      {cardMetrics.length > 0 && (
        <CardContent className="mt-auto px-3.5 pb-3 pt-0">
          <PluginCardItemSurface>
            <PluginCardItemGrid
              emphasizeValues
              items={cardMetrics.map((metric) => ({
                key: metric.label,
                label: metric.label,
                value: metric.value,
              }))}
            />
          </PluginCardItemSurface>
        </CardContent>
      )}
      {showFallbackContent && (
        <CardContent className="mt-auto px-3.5 pb-3 pt-0">
          <PluginCardItemSurface>{children}</PluginCardItemSurface>
        </CardContent>
      )}
    </Card>
  );
}
