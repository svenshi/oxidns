"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Loader2,
  Plus,
  RefreshCw,
  Save,
  ShieldCheck,
  Trash2,
} from "lucide-react";
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
import {
  fetchCronStatus,
  fetchDownloadStatus,
  fetchProviderStatus,
  reloadProvider,
  runCronJob,
  runDownload,
  type CronStatusResponse,
  type DownloadItemStatus,
  type DownloadStatusResponse,
  type ProviderStatusResponse,
} from "@/lib/oxidns-api";
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

const STANDARD_FILTER_SUBSCRIPTION_DIR =
  "./data/standard-filter-subscriptions";
const FILTER_REFRESH_JOB = "refresh_filter_subscriptions";

interface SubscriptionRuntimeState {
  download?: DownloadStatusResponse | null;
  cron?: CronStatusResponse | null;
  provider?: ProviderStatusResponse | null;
}

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

function subscriptionFileName(subscription: StandardSubscription) {
  const safe = subscription.id
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "_")
    .replace(/^_+|_+$/g, "");
  return `${safe || "subscription"}.txt`;
}

function subscriptionFilePath(subscription: StandardSubscription) {
  return `${STANDARD_FILTER_SUBSCRIPTION_DIR}/${subscriptionFileName(subscription)}`;
}

function findDownloadItem(
  status: DownloadStatusResponse | null | undefined,
  subscription: StandardSubscription,
) {
  const path = subscriptionFilePath(subscription);
  return status?.items.find((item) => item.path === path) ?? null;
}

function tagAvailable(tags: string[], tag: string) {
  return tags.includes(tag);
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
  const standardLastGenerated = useAppStore((s) => s.standardLastGenerated);
  const saveStandardSettings = useAppStore((s) => s.saveStandardSettings);
  const isConfigSaving = useAppStore((s) => s.isConfigSaving);
  const isApplying = useAppStore((s) => s.isApplying);
  const pluginMetrics = useAppStore((s) => s.pluginMetrics);
  const { t, formatNumber, formatDateTime } = useI18n();
  const capabilities = useMemo(
    () => standardFilteringCapabilityMap(buildInfo),
    [buildInfo],
  );
  const [draftSettings, setDraftSettings] =
    useState<StandardModeSettings | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [runtimeState, setRuntimeState] = useState<SubscriptionRuntimeState>(
    {},
  );
  const [runtimeLoading, setRuntimeLoading] = useState(false);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);
  const [refreshingSubscriptions, setRefreshingSubscriptions] = useState(false);
  const [refreshMessage, setRefreshMessage] = useState<string | null>(null);
  const settings = draftSettings ?? storeSettings;
  const filteringTags = useMemo(
    () => standardLastGenerated?.tagMap.filtering ?? [],
    [standardLastGenerated],
  );
  const runtimeTags = useMemo(
    () => ({
      download: tagAvailable(filteringTags, "standard_filter_download")
        ? "standard_filter_download"
        : null,
      cron: tagAvailable(filteringTags, "standard_filter_cron")
        ? "standard_filter_cron"
        : null,
      provider: tagAvailable(filteringTags, "standard_ad_rules")
        ? "standard_ad_rules"
        : null,
      reload: tagAvailable(filteringTags, "standard_filter_reload")
        ? "standard_filter_reload"
        : null,
    }),
    [filteringTags],
  );
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
  const hasRuntimeTags =
    runtimeTags.download !== null ||
    runtimeTags.cron !== null ||
    runtimeTags.provider !== null;

  const loadRuntimeStatus = useCallback(async () => {
    if (!hasRuntimeTags) {
      setRuntimeState({});
      setRuntimeError(null);
      return;
    }
    setRuntimeLoading(true);
    setRuntimeError(null);
    const [downloadResult, cronResult, providerResult] = await Promise.all([
      runtimeTags.download
        ? fetchDownloadStatus(runtimeTags.download)
        : Promise.resolve(null),
      runtimeTags.cron ? fetchCronStatus(runtimeTags.cron) : Promise.resolve(null),
      runtimeTags.provider
        ? fetchProviderStatus(runtimeTags.provider)
        : Promise.resolve(null),
    ].map((promise) => promise.catch((error) => error)));

    const failures = [downloadResult, cronResult, providerResult].filter(
      (result) => result instanceof Error,
    ) as Error[];
    setRuntimeState({
      download:
        downloadResult instanceof Error
          ? null
          : (downloadResult as DownloadStatusResponse | null),
      cron:
        cronResult instanceof Error
          ? null
          : (cronResult as CronStatusResponse | null),
      provider:
        providerResult instanceof Error
          ? null
          : (providerResult as ProviderStatusResponse | null),
    });
    setRuntimeError(failures[0]?.message ?? null);
    setRuntimeLoading(false);
  }, [hasRuntimeTags, runtimeTags]);

  useEffect(() => {
    const timer = window.setTimeout(() => void loadRuntimeStatus(), 0);
    return () => window.clearTimeout(timer);
  }, [loadRuntimeStatus]);

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

  const handleRefreshSubscriptions = async () => {
    setRefreshingSubscriptions(true);
    setRefreshMessage(null);
    setRuntimeError(null);
    try {
      if (runtimeTags.cron) {
        const result = await runCronJob(runtimeTags.cron, FILTER_REFRESH_JOB);
        if (!result.ok) {
          throw new Error(
            result.last_error ||
              t(WEBUI.standardFiltering.subscriptionRefreshFailed),
          );
        }
      } else if (runtimeTags.download && runtimeTags.provider && runtimeTags.reload) {
        const result = await runDownload(runtimeTags.download);
        if (!result.ok) {
          throw new Error(
            result.results.find((item) => item.last_error)?.last_error ||
              t(WEBUI.standardFiltering.subscriptionRefreshFailed),
          );
        }
        await reloadProvider(runtimeTags.provider);
      } else {
        throw new Error(
          t(WEBUI.standardFiltering.subscriptionRuntimeNotApplied),
        );
      }
      await loadRuntimeStatus();
      setRefreshMessage(t(WEBUI.standardFiltering.subscriptionRefreshSuccess));
    } catch (error) {
      setRuntimeError(error instanceof Error ? error.message : String(error));
      setRefreshMessage(null);
    } finally {
      setRefreshingSubscriptions(false);
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
              <div className="flex flex-wrap justify-end gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleRefreshSubscriptions}
                  disabled={
                    refreshingSubscriptions ||
                    enabledSubscriptions.length === 0 ||
                    !hasRuntimeTags
                  }
                >
                  {refreshingSubscriptions ? (
                    <Loader2 className="size-4 animate-spin" />
                  ) : (
                    <RefreshCw className="size-4" />
                  )}
                  {t(WEBUI.standardFiltering.refreshSubscriptions)}
                </Button>
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
              </div>
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
              <SubscriptionRuntimePanel
                subscriptions={settings.filtering.subscriptions}
                runtimeState={runtimeState}
                loading={runtimeLoading}
                error={runtimeError}
                refreshMessage={refreshMessage}
                hasRuntimeTags={hasRuntimeTags}
                formatNumber={formatNumber}
                formatDateTime={formatDateTime}
              />
            </CardContent>
          </Card>
        </div>
      </main>
    </>
  );
}

function SubscriptionRuntimePanel({
  subscriptions,
  runtimeState,
  loading,
  error,
  refreshMessage,
  hasRuntimeTags,
  formatNumber,
  formatDateTime,
}: {
  subscriptions: StandardSubscription[];
  runtimeState: SubscriptionRuntimeState;
  loading: boolean;
  error: string | null;
  refreshMessage: string | null;
  hasRuntimeTags: boolean;
  formatNumber: (value: number, options?: Intl.NumberFormatOptions) => string;
  formatDateTime: (
    value: Date | number | string,
    options?: Intl.DateTimeFormatOptions,
  ) => string;
}) {
  const { t } = useI18n();
  if (subscriptions.length === 0) return null;
  const refreshJob = runtimeState.cron?.jobs.find(
    (job) => job.name === FILTER_REFRESH_JOB,
  );
  const ruleStats = runtimeState.provider?.rule_stats;

  return (
    <div className="space-y-3 rounded-lg border bg-muted/20 p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="text-sm font-medium">
            {t(WEBUI.standardFiltering.subscriptionStatusTitle)}
          </div>
          <p className="mt-1 text-xs text-muted-foreground">
            {t(WEBUI.standardFiltering.subscriptionStatusDescription)}
          </p>
        </div>
        {loading ? (
          <Badge variant="secondary">
            <Loader2 className="mr-1 size-3 animate-spin" />
            {t(WEBUI.common.loading)}
          </Badge>
        ) : null}
      </div>

      {!hasRuntimeTags ? (
        <div className="rounded-lg border border-dashed p-3 text-sm text-muted-foreground">
          {t(WEBUI.standardFiltering.subscriptionRuntimeNotApplied)}
        </div>
      ) : null}
      {error ? (
        <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
          {error}
        </div>
      ) : null}
      {refreshMessage ? (
        <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-3 text-sm text-emerald-700 dark:text-emerald-300">
          {refreshMessage}
        </div>
      ) : null}

      <div className="grid gap-3 md:grid-cols-2">
        {subscriptions.map((subscription) => {
          const item = findDownloadItem(runtimeState.download, subscription);
          return (
            <SubscriptionRuntimeItem
              key={subscription.id}
              subscription={subscription}
              item={item}
              formatNumber={formatNumber}
              formatDateTime={formatDateTime}
            />
          );
        })}
      </div>

      <div className="grid gap-2 md:grid-cols-2">
        <RuntimeFact
          label={t(WEBUI.standardFiltering.providerTotalRules)}
          value={numberOrPlaceholder(ruleStats?.total_rules, t, formatNumber)}
        />
        <RuntimeFact
          label={t(WEBUI.standardFiltering.providerSupportedRules)}
          value={numberOrPlaceholder(
            ruleStats?.supported_rules,
            t,
            formatNumber,
          )}
        />
        <RuntimeFact
          label={t(WEBUI.standardFiltering.providerSkippedRules)}
          value={numberOrPlaceholder(ruleStats?.skipped_rules, t, formatNumber)}
        />
        <RuntimeFact
          label={t(WEBUI.standardFiltering.providerExceptionRules)}
          value={numberOrPlaceholder(
            ruleStats?.exception_rules,
            t,
            formatNumber,
          )}
        />
        <RuntimeFact
          label={t(WEBUI.standardFiltering.cronNextRun)}
          value={timeOrPlaceholder(
            refreshJob?.next_run_ms,
            t,
            formatDateTime,
          )}
        />
        <RuntimeFact
          label={t(WEBUI.standardFiltering.cronLastSuccess)}
          value={timeOrPlaceholder(
            refreshJob?.last_success_ms,
            t,
            formatDateTime,
          )}
        />
      </div>
    </div>
  );
}

function SubscriptionRuntimeItem({
  subscription,
  item,
  formatNumber,
  formatDateTime,
}: {
  subscription: StandardSubscription;
  item: DownloadItemStatus | null;
  formatNumber: (value: number, options?: Intl.NumberFormatOptions) => string;
  formatDateTime: (
    value: Date | number | string,
    options?: Intl.DateTimeFormatOptions,
  ) => string;
}) {
  const { t } = useI18n();
  return (
    <div className="space-y-2 rounded-lg border bg-card p-3 text-sm">
      <div className="flex min-w-0 items-center justify-between gap-2">
        <span className="truncate font-medium">{subscription.name}</span>
        <Badge variant={subscription.enabled ? "default" : "secondary"}>
          {subscription.enabled ? t(WEBUI.common.enabled) : t(WEBUI.common.disabled)}
        </Badge>
      </div>
      <RuntimeFact
        label={t(WEBUI.standardFiltering.subscriptionFile)}
        value={item?.path ?? subscriptionFilePath(subscription)}
      />
      <RuntimeFact
        label={t(WEBUI.standardFiltering.subscriptionFileSize)}
        value={
          item?.file.exists
            ? formatBytes(item.file.size_bytes, t, formatNumber)
            : t(WEBUI.standardFiltering.subscriptionFileMissing)
        }
      />
      <RuntimeFact
        label={t(WEBUI.standardFiltering.subscriptionFileModified)}
        value={timeOrPlaceholder(
          item?.file.modified_at_ms,
          t,
          formatDateTime,
        )}
      />
      <RuntimeFact
        label={t(WEBUI.standardFiltering.subscriptionLastSuccess)}
        value={timeOrPlaceholder(item?.last_success_ms, t, formatDateTime)}
      />
      <RuntimeFact
        label={t(WEBUI.standardFiltering.subscriptionLastError)}
        value={item?.last_error || t(WEBUI.common.empty)}
      />
    </div>
  );
}

function RuntimeFact({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex min-h-8 min-w-0 items-center justify-between gap-3 rounded-md border bg-background px-2 text-xs">
      <span className="shrink-0 text-muted-foreground">{label}</span>
      <span className="min-w-0 truncate text-right font-medium">{value}</span>
    </div>
  );
}

function numberOrPlaceholder(
  value: number | null | undefined,
  t: (key: string) => string,
  formatNumber: (value: number, options?: Intl.NumberFormatOptions) => string,
) {
  return typeof value === "number"
    ? formatNumber(value)
    : t(WEBUI.standardFiltering.metricUnavailable);
}

function timeOrPlaceholder(
  value: number | null | undefined,
  t: (key: string) => string,
  formatDateTime: (
    value: Date | number | string,
    options?: Intl.DateTimeFormatOptions,
  ) => string,
) {
  return typeof value === "number"
    ? formatDateTime(value)
    : t(WEBUI.common.empty);
}

function formatBytes(
  value: number | null | undefined,
  t: (key: string) => string,
  formatNumber: (value: number, options?: Intl.NumberFormatOptions) => string,
) {
  if (typeof value !== "number") return t(WEBUI.common.empty);
  if (value < 1024) return `${formatNumber(value)} B`;
  if (value < 1024 * 1024) {
    return `${formatNumber(value / 1024, { maximumFractionDigits: 1 })} KiB`;
  }
  return `${formatNumber(value / 1024 / 1024, {
    maximumFractionDigits: 1,
  })} MiB`;
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
