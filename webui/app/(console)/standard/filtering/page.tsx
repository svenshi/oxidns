"use client";

import { useMemo, useState } from "react";
import { Loader2, Plus, Save, ShieldCheck, Trash2 } from "lucide-react";
import { AppHeader } from "@/components/shell/app-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import type {
  StandardFilteringSettings,
  StandardModeSettings,
  StandardSubscription,
} from "@/lib/standard-mode/types";
import {
  normalizeStandardFilteringSettings,
  standardFilteringCapabilityMap,
  validateStandardFilteringSettings,
  type StandardFilteringValidationIssue,
} from "@/lib/standard-mode/validation";
import { useAppStore } from "@/lib/store";

const blockResponseLabelKeys: Record<
  StandardFilteringSettings["blockResponse"],
  string
> = {
  null_ip: WEBUI.standardFiltering.responseNullIp,
  nxdomain: WEBUI.standardFiltering.responseNxdomain,
  refused: WEBUI.standardFiltering.responseRefused,
};

function lines(value: string) {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const line of value.split("\n")) {
    const next = line.trim();
    if (!next || seen.has(next)) continue;
    seen.add(next);
    result.push(next);
  }
  return result;
}

function createSubscriptionId(subscriptions: StandardSubscription[]) {
  const used = new Set(subscriptions.map((item) => item.id));
  let index = subscriptions.length + 1;
  let id = `subscription_${index}`;
  while (used.has(id)) {
    index += 1;
    id = `subscription_${index}`;
  }
  return id;
}

function createSubscription(
  subscriptions: StandardSubscription[],
): StandardSubscription {
  const id = createSubscriptionId(subscriptions);
  return {
    id,
    name: id,
    url: "",
    enabled: true,
    updateIntervalHours: 24,
  };
}

function numberValue(value: string, fallback: number) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function sumMetric(
  series: ReturnType<typeof useAppStore.getState>["pluginMetrics"][string],
  name: string,
) {
  return (
    series
      ?.filter((item) => item.name === name && Number.isFinite(item.value))
      .reduce((sum, item) => sum + item.value, 0) ?? null
  );
}

export default function StandardFilteringPage() {
  const storeSettings = useAppStore((s) => s.standardSettings);
  const buildInfo = useAppStore((s) => s.buildInfo);
  const saveStandardSettings = useAppStore((s) => s.saveStandardSettings);
  const isConfigSaving = useAppStore((s) => s.isConfigSaving);
  const isApplying = useAppStore((s) => s.isApplying);
  const pluginMetrics = useAppStore((s) => s.pluginMetrics);
  const { t, formatNumber } = useI18n();
  const capabilities = useMemo(
    () => standardFilteringCapabilityMap(buildInfo),
    [buildInfo],
  );
  const [draftSettings, setDraftSettings] =
    useState<StandardModeSettings | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const settings = draftSettings ?? storeSettings;
  const validationIssues = useMemo(
    () => validateStandardFilteringSettings(settings, buildInfo),
    [settings, buildInfo],
  );
  const isBusy = isConfigSaving || isApplying;
  const canSave = validationIssues.length === 0 && !isBusy;
  const enabledSubscriptions = settings.filtering.subscriptions.filter(
    (item) => item.enabled,
  );
  const blockedTotal = sumMetric(
    pluginMetrics["standard_blocked"],
    "blackhole_block_total",
  );

  const setPartial = (patch: Partial<StandardModeSettings>) => {
    setSaveError(null);
    setDraftSettings((current) => ({ ...(current ?? settings), ...patch }));
  };

  const setFiltering = (patch: Partial<StandardFilteringSettings>) => {
    setPartial({
      filtering: { ...settings.filtering, ...patch },
    });
  };

  const updateSubscription = (
    subscriptionId: string,
    patch: Partial<StandardSubscription>,
  ) => {
    setFiltering({
      subscriptions: settings.filtering.subscriptions.map((subscription) =>
        subscription.id === subscriptionId
          ? { ...subscription, ...patch }
          : subscription,
      ),
    });
  };

  const removeSubscription = (subscriptionId: string) => {
    setFiltering({
      subscriptions: settings.filtering.subscriptions.filter(
        (subscription) => subscription.id !== subscriptionId,
      ),
    });
  };

  const handleSave = async () => {
    const nextSettings = normalizeStandardFilteringSettings(settings);
    const issues = validateStandardFilteringSettings(nextSettings, buildInfo);
    if (issues.length > 0) return;
    setSaveError(null);
    try {
      await saveStandardSettings(nextSettings, { apply: true });
      setDraftSettings(nextSettings);
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <>
      <AppHeader title={t(WEBUI.standardFiltering.title)} />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-6xl space-y-6">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <h1 className="text-xl font-semibold tracking-tight">
                {t(WEBUI.standardFiltering.title)}
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                {t(WEBUI.standardFiltering.description)}
              </p>
            </div>
            <Button onClick={handleSave} disabled={!canSave}>
              {isBusy ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Save className="size-4" />
              )}
              {isBusy
                ? t(WEBUI.standardFiltering.savingApplying)
                : t(WEBUI.standardFiltering.saveApply)}
            </Button>
          </div>

          {validationIssues.length > 0 || saveError ? (
            <ValidationPanel issues={validationIssues} saveError={saveError} />
          ) : null}

          <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_320px]">
            <div className="space-y-6">
              <Card>
                <CardHeader className="flex flex-row items-center justify-between space-y-0">
                  <CardTitle className="flex items-center gap-2 text-base">
                    <ShieldCheck className="size-4" />
                    {t(WEBUI.standardFiltering.overviewTitle)}
                  </CardTitle>
                  {!capabilities.adRules || !capabilities.blackHole ? (
                    <Badge variant="secondary">
                      {t(WEBUI.standardFiltering.unsupportedFiltering)}
                    </Badge>
                  ) : null}
                </CardHeader>
                <CardContent className="grid gap-5 md:grid-cols-2">
                  <Label className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal">
                    {t(WEBUI.standardFiltering.enabled)}
                    <Switch
                      checked={settings.filtering.enabled}
                      onCheckedChange={(checked) =>
                        setFiltering({ enabled: checked })
                      }
                    />
                  </Label>
                  <div className="space-y-2">
                    <Label htmlFor="standard-filtering-block-response">
                      {t(WEBUI.standardFiltering.blockResponse)}
                    </Label>
                    <Select
                      value={settings.filtering.blockResponse}
                      onValueChange={(value) =>
                        setFiltering({
                          blockResponse:
                            value as StandardFilteringSettings["blockResponse"],
                        })
                      }
                    >
                      <SelectTrigger
                        id="standard-filtering-block-response"
                        className="w-full"
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {(["null_ip", "nxdomain", "refused"] as const).map(
                          (value) => (
                            <SelectItem key={value} value={value}>
                              {t(blockResponseLabelKeys[value])}
                            </SelectItem>
                          ),
                        )}
                      </SelectContent>
                    </Select>
                  </div>
                  {settings.filtering.subscriptions.length > 0 &&
                  !capabilities.subscriptionRuntime ? (
                    <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-700 md:col-span-2 dark:text-amber-300">
                      {t(WEBUI.standardFiltering.subscriptionRuntimeUnavailable)}
                    </div>
                  ) : null}
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle className="text-base">
                    {t(WEBUI.standardFiltering.manualBlockTitle)}
                  </CardTitle>
                  <p className="text-sm text-muted-foreground">
                    {t(WEBUI.standardFiltering.manualBlockDescription)}
                  </p>
                </CardHeader>
                <CardContent>
                  <Textarea
                    value={settings.filtering.blockRules.join("\n")}
                    disabled={!settings.filtering.enabled}
                    onChange={(event) =>
                      setFiltering({ blockRules: lines(event.target.value) })
                    }
                    rows={7}
                    placeholder={"||example.com^\n||ads.example.net^"}
                  />
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle className="text-base">
                    {t(WEBUI.standardFiltering.manualAllowTitle)}
                  </CardTitle>
                  <p className="text-sm text-muted-foreground">
                    {t(WEBUI.standardFiltering.manualAllowDescription)}
                  </p>
                </CardHeader>
                <CardContent>
                  <Textarea
                    value={settings.filtering.allowRules.join("\n")}
                    disabled={!settings.filtering.enabled}
                    onChange={(event) =>
                      setFiltering({ allowRules: lines(event.target.value) })
                    }
                    rows={6}
                    placeholder={"@@||allowed.example.com^\n||safe.example.net^"}
                  />
                </CardContent>
              </Card>
            </div>

            <Card className="h-fit">
              <CardHeader>
                <CardTitle className="text-base">
                  {t(WEBUI.standardFiltering.statsTitle)}
                </CardTitle>
              </CardHeader>
              <CardContent className="grid gap-3">
                <StatRow
                  label={t(WEBUI.standardFiltering.blockRuleCount)}
                  value={formatNumber(settings.filtering.blockRules.length)}
                />
                <StatRow
                  label={t(WEBUI.standardFiltering.allowRuleCount)}
                  value={formatNumber(settings.filtering.allowRules.length)}
                />
                <StatRow
                  label={t(WEBUI.standardFiltering.enabledSubscriptionCount)}
                  value={formatNumber(enabledSubscriptions.length)}
                />
                <StatRow
                  label={t(WEBUI.standardFiltering.blockResponseMetric)}
                  value={t(
                    blockResponseLabelKeys[settings.filtering.blockResponse],
                  )}
                />
                <StatRow
                  label={t(WEBUI.standardFiltering.blockedTotal)}
                  value={
                    blockedTotal == null
                      ? t(WEBUI.standardFiltering.metricUnavailable)
                      : formatNumber(blockedTotal)
                  }
                />
              </CardContent>
            </Card>
          </div>

          <Card>
            <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
              <div>
                <CardTitle className="text-base">
                  {t(WEBUI.standardFiltering.subscriptionsTitle)}
                </CardTitle>
                <p className="mt-1 text-sm text-muted-foreground">
                  {t(WEBUI.standardFiltering.subscriptionsDescription)}
                </p>
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={() =>
                  setFiltering({
                    subscriptions: [
                      ...settings.filtering.subscriptions,
                      createSubscription(settings.filtering.subscriptions),
                    ],
                  })
                }
              >
                <Plus className="size-4" />
                {t(WEBUI.standardFiltering.addSubscription)}
              </Button>
            </CardHeader>
            <CardContent className="space-y-3">
              {settings.filtering.subscriptions.map((subscription) => (
                <SubscriptionEditor
                  key={subscription.id}
                  subscription={subscription}
                  onChange={(patch) =>
                    updateSubscription(subscription.id, patch)
                  }
                  onRemove={() => removeSubscription(subscription.id)}
                />
              ))}
              {settings.filtering.subscriptions.length === 0 ? (
                <div className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
                  {t(WEBUI.standardFiltering.subscriptionsDescription)}
                </div>
              ) : null}
            </CardContent>
          </Card>
        </div>
      </main>
    </>
  );
}

function SubscriptionEditor({
  subscription,
  onChange,
  onRemove,
}: {
  subscription: StandardSubscription;
  onChange: (patch: Partial<StandardSubscription>) => void;
  onRemove: () => void;
}) {
  const { t } = useI18n();
  return (
    <div className="rounded-lg border bg-card/40 p-4">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <Label className="text-sm font-normal">
          <Switch
            checked={subscription.enabled}
            onCheckedChange={(checked) => onChange({ enabled: checked })}
          />
          {t(WEBUI.standardFiltering.subscriptionEnabled)}
        </Label>
        <Button type="button" variant="ghost" size="sm" onClick={onRemove}>
          <Trash2 className="size-4" />
          {t(WEBUI.standardFiltering.removeSubscription)}
        </Button>
      </div>
      <div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_minmax(0,2fr)_180px]">
        <div className="space-y-2">
          <Label htmlFor={`${subscription.id}-name`}>
            {t(WEBUI.standardFiltering.subscriptionName)}
          </Label>
          <Input
            id={`${subscription.id}-name`}
            value={subscription.name}
            onChange={(event) => onChange({ name: event.target.value })}
            placeholder="EasyList"
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor={`${subscription.id}-url`}>
            {t(WEBUI.standardFiltering.subscriptionUrl)}
          </Label>
          <Input
            id={`${subscription.id}-url`}
            value={subscription.url}
            onChange={(event) => onChange({ url: event.target.value })}
            placeholder="https://example.com/filter.txt"
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor={`${subscription.id}-interval`}>
            {t(WEBUI.standardFiltering.subscriptionInterval)}
          </Label>
          <Input
            id={`${subscription.id}-interval`}
            type="number"
            min={1}
            value={subscription.updateIntervalHours}
            onChange={(event) =>
              onChange({
                updateIntervalHours: Math.max(
                  1,
                  Math.trunc(
                    numberValue(
                      event.target.value,
                      subscription.updateIntervalHours,
                    ),
                  ),
                ),
              })
            }
          />
        </div>
      </div>
    </div>
  );
}

function StatRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex min-h-10 items-center justify-between gap-3 rounded-lg border px-3 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <span className="truncate font-medium">{value}</span>
    </div>
  );
}

function ValidationPanel({
  issues,
  saveError,
}: {
  issues: StandardFilteringValidationIssue[];
  saveError: string | null;
}) {
  const { t } = useI18n();
  return (
    <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-4 text-sm text-destructive">
      <div className="font-medium">
        {t(WEBUI.standardFiltering.validationTitle)}
      </div>
      <ul className="mt-2 list-disc space-y-1 pl-5">
        {issues.map((issue, index) => (
          <li key={`${issue.field}-${issue.code}-${index}`}>
            {validationMessage(issue, t)}
          </li>
        ))}
        {saveError ? <li>{saveError}</li> : null}
      </ul>
    </div>
  );
}

function validationMessage(
  issue: StandardFilteringValidationIssue,
  t: (key: string) => string,
) {
  if (issue.code === "capability_required") {
    return t(WEBUI.standardFiltering.validationCapabilityRequired);
  }
  if (issue.code === "rule_source_required") {
    return t(WEBUI.standardFiltering.validationRuleSourceRequired);
  }
  if (issue.code === "subscription_runtime_required") {
    return t(WEBUI.standardFiltering.validationSubscriptionRuntimeRequired);
  }
  if (issue.code === "subscription_name_required") {
    return t(WEBUI.standardFiltering.validationSubscriptionNameRequired);
  }
  if (issue.code === "subscription_url_required") {
    return t(WEBUI.standardFiltering.validationSubscriptionUrlRequired);
  }
  if (issue.code === "subscription_url_invalid") {
    return t(WEBUI.standardFiltering.validationSubscriptionUrlInvalid);
  }
  return t(WEBUI.standardFiltering.validationSubscriptionIntervalInvalid);
}
