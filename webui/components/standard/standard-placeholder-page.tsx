"use client";

import type { LucideIcon } from "lucide-react";
import { AppHeader } from "@/components/shell/app-header";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

interface StandardPlaceholderMetric {
  label: string;
  value: number;
}

interface StandardPlaceholderPageProps {
  titleKey: string;
  descriptionKey: string;
  emptyTitleKey: string;
  emptyDescriptionKey: string;
  badgeKey: string;
  icon: LucideIcon;
  metrics: StandardPlaceholderMetric[];
}

export function StandardPlaceholderPage({
  titleKey,
  descriptionKey,
  emptyTitleKey,
  emptyDescriptionKey,
  badgeKey,
  icon: Icon,
  metrics,
}: StandardPlaceholderPageProps) {
  const { t, formatNumber } = useI18n();
  const title = t(titleKey);

  return (
    <>
      <AppHeader title={title} />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-5xl space-y-6">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <h1 className="text-xl font-semibold tracking-tight">{title}</h1>
              <p className="mt-1 max-w-3xl text-sm text-muted-foreground">
                {t(descriptionKey)}
              </p>
            </div>
            <Badge variant="secondary">{t(badgeKey)}</Badge>
          </div>

          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {metrics.map((metric) => (
              <Card key={metric.label}>
                <CardHeader className="pb-2">
                  <CardTitle className="text-sm font-medium text-muted-foreground">
                    {metric.label}
                  </CardTitle>
                </CardHeader>
                <CardContent>
                  <div className="text-2xl font-semibold">
                    {formatNumber(metric.value)}
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>

          <Card>
            <CardContent className="flex flex-col items-center justify-center gap-3 p-10 text-center">
              <div className="flex size-11 items-center justify-center rounded-md bg-muted text-muted-foreground">
                <Icon className="size-5" />
              </div>
              <div className="space-y-1">
                <h2 className="text-base font-medium">{t(emptyTitleKey)}</h2>
                <p className="max-w-2xl text-sm text-muted-foreground">
                  {t(emptyDescriptionKey)}
                </p>
              </div>
              <p className="text-xs text-muted-foreground">
                {t(WEBUI.standardShell.phase3ReadOnly)}
              </p>
            </CardContent>
          </Card>
        </div>
      </main>
    </>
  );
}
