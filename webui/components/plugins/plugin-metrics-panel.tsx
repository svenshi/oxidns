"use client";

import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { useAppStore } from "@/lib/store";
import {
  formatMetricValue,
  groupMetricRows,
  type MetricRow,
} from "@/lib/metrics";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

export function PluginMetricsPanel({ tag }: { tag: string }) {
  const { locale, t } = useI18n();
  const series = useAppStore((s) => s.pluginMetrics[tag]);
  if (!series || series.length === 0) return null;

  const rows = groupMetricRows(series, locale);

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between gap-3 p-4 pb-2">
        <CardTitle className="text-sm">
          {t(WEBUI.plugins.metricsTitle)}
        </CardTitle>
        <Badge variant="outline" className="font-mono text-xs">
          {t(WEBUI.plugins.metricCount, { count: rows.length })}
        </Badge>
      </CardHeader>
      <CardContent className="space-y-2 p-4 pt-0">
        {rows.map((row) => (
          <MetricRowItem key={row.name} row={row} locale={locale} />
        ))}
      </CardContent>
    </Card>
  );
}

function MetricRowItem({
  row,
  locale,
}: {
  row: MetricRow;
  locale: ReturnType<typeof useI18n>["locale"];
}) {
  const { t } = useI18n();
  return (
    <div className="rounded-md border border-border/70 bg-background/60 px-3 py-2">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex min-w-0 flex-wrap items-center gap-1.5">
            <span className="text-sm font-medium">{row.label}</span>
            {row.kind && (
              <Badge variant="outline" className="h-5 px-1.5 font-mono text-xs">
                {row.kind}
              </Badge>
            )}
            {row.highValue && (
              <Badge variant="secondary" className="h-5 px-1.5 text-xs">
                {t(WEBUI.plugins.cardBadge)}
              </Badge>
            )}
          </div>
          <div className="mt-0.5 truncate font-mono text-xs text-muted-foreground">
            {row.name}
          </div>
          {row.help && (
            <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">
              {row.help}
            </p>
          )}
        </div>
        <span className="shrink-0 font-mono text-sm font-semibold tabular-nums">
          {formatMetricValue(row.total, locale, { metricName: row.name })}
        </span>
      </div>
      {row.breakdown.length > 0 && (
        <div className="mt-1.5 space-y-1 border-t border-border/60 pt-1.5">
          {row.breakdown.map((item) => (
            <div
              key={item.key}
              className="flex items-baseline justify-between gap-3 text-xs"
            >
              <span className="truncate font-mono text-muted-foreground">
                {item.key}
              </span>
              <span className="shrink-0 font-mono tabular-nums">
                {formatMetricValue(item.value, locale, {
                  metricName: row.name,
                })}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
