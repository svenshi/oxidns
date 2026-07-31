"use client";

import Link from "next/link";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Database,
  GitBranch,
  Loader2,
  Plus,
  RefreshCw,
  Route,
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
  fetchDownloadStatus,
  fetchProviderStatus,
  runCronJob,
  type DownloadStatusResponse,
  type ProviderStatusResponse,
} from "@/lib/oxidns-api";
import {
  selectStandardPathReferences,
  type StandardEntityReference,
  type StandardReferenceKind,
} from "@/lib/standard-mode/selectors";
import type {
  StandardModeSettings,
  StandardResolutionPath,
  StandardRuleDataSettings,
  StandardRuleDataSource,
  StandardRoutingRule,
} from "@/lib/standard-mode/types";
import {
  normalizeStandardRoutingSettings,
  standardRoutingCapabilityMap,
  validateStandardRoutingSettings,
  type StandardRoutingValidationIssue,
} from "@/lib/standard-mode/validation";
import { useAppStore } from "@/lib/store";

type RoutingConditionType = Extract<
  StandardRoutingRule["condition"]["type"],
  "domain" | "suffix" | "keyword" | "client_cidr" | "qtype"
>;

type PathPolicy = "inherit" | "enabled" | "disabled";
type RuleDataRoleKey = keyof StandardRuleDataSettings;

type RuleDataRoleRuntimeKey =
  | "domestic_domains"
  | "foreign_domains"
  | "domestic_ips"
  | "direct_domains"
  | "remote_domains"
  | "ddns_domains";

interface RuleDataRuntimeState {
  providers: Partial<Record<RuleDataRoleRuntimeKey, ProviderStatusResponse | null>>;
  subscriptions: Record<string, DownloadStatusResponse | null>;
}

const ruleDataRoles: RuleDataRoleKey[] = [
  "domesticDomains",
  "foreignDomains",
  "domesticIps",
  "directDomains",
  "remoteDomains",
  "ddnsDomains",
];

const ruleDataRuntimeKeys: Record<RuleDataRoleKey, RuleDataRoleRuntimeKey> = {
  domesticDomains: "domestic_domains",
  foreignDomains: "foreign_domains",
  domesticIps: "domestic_ips",
  directDomains: "direct_domains",
  remoteDomains: "remote_domains",
  ddnsDomains: "ddns_domains",
};

const ruleDataRoleLabelKeys: Record<RuleDataRoleKey, string> = {
  domesticDomains: WEBUI.standardRouting.roleDomesticDomains,
  foreignDomains: WEBUI.standardRouting.roleForeignDomains,
  domesticIps: WEBUI.standardRouting.roleDomesticIps,
  directDomains: WEBUI.standardRouting.roleDirectDomains,
  remoteDomains: WEBUI.standardRouting.roleRemoteDomains,
  ddnsDomains: WEBUI.standardRouting.roleDdnsDomains,
};

const ruleDataSourceTypeLabelKeys: Record<
  StandardRuleDataSource["type"],
  string
> = {
  manual: WEBUI.standardRouting.sourceTypeManual,
  local_file: WEBUI.standardRouting.sourceTypeLocalFile,
  subscription: WEBUI.standardRouting.sourceTypeSubscription,
  native_dat: WEBUI.standardRouting.sourceTypeNativeDat,
};

const conditionLabelKeys: Record<RoutingConditionType, string> = {
  domain: WEBUI.standardRouting.conditionDomain,
  suffix: WEBUI.standardRouting.conditionSuffix,
  keyword: WEBUI.standardRouting.conditionKeyword,
  client_cidr: WEBUI.standardRouting.conditionClientCidr,
  qtype: WEBUI.standardRouting.conditionQtype,
};

const policyLabelKeys: Record<PathPolicy, string> = {
  inherit: WEBUI.standardRouting.policyInherit,
  enabled: WEBUI.standardRouting.policyEnabled,
  disabled: WEBUI.standardRouting.policyDisabled,
};

const smartResponsePolicyLabelKeys: Record<
  keyof StandardModeSettings["smartRouting"]["responsePolicy"],
  string
> = {
  domesticIpMismatch: WEBUI.standardRouting.responseDomesticIpMismatch,
  cnameOnly: WEBUI.standardRouting.responseCnameOnly,
  nodata: WEBUI.standardRouting.responseNodata,
  nxdomain: WEBUI.standardRouting.responseNxdomain,
  servfail: WEBUI.standardRouting.responseServfail,
  timeout: WEBUI.standardRouting.responseTimeout,
  transportFailure: WEBUI.standardRouting.responseTransportFailure,
};

const referenceKindLabelKeys: Record<StandardReferenceKind, string> = {
  path: WEBUI.standardRouting.referencePath,
  routing_rule: WEBUI.standardRouting.referenceRoutingRule,
  exception: WEBUI.standardRouting.referenceException,
  device: WEBUI.standardRouting.referenceDevice,
  ddns: WEBUI.standardRouting.referenceDdns,
};

const conditionPlaceholders: Record<RoutingConditionType, string> = {
  domain: "example.com",
  suffix: "example.com",
  keyword: "game",
  client_cidr: "192.168.1.0/24",
  qtype: "A\nAAAA",
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

function nextId(prefix: string, existing: string[]) {
  const used = new Set(existing);
  let index = existing.length + 1;
  let id = `${prefix}_${index}`;
  while (used.has(id)) {
    index += 1;
    id = `${prefix}_${index}`;
  }
  return id;
}

function createPath(paths: StandardResolutionPath[]): StandardResolutionPath {
  const id = nextId(
    "path",
    paths.map((path) => path.id),
  );
  return {
    id,
    name: id,
    upstreamGroupId: paths[0]?.upstreamGroupId ?? "default",
    filtering: "inherit",
    cache: "inherit",
    queryLog: "inherit",
    dualStack: "inherit",
    ipSelection: {
      enabled: false,
      selectionMode: "first_success",
      probeMethods: ["tcp:443", "tcp:80"],
      probeStaggerMs: 200,
      probeTimeoutMs: 600,
      maxWaitMs: 1000,
      topN: 1,
      dnssecPolicy: "reorder_only",
      maxParallelProbes: 256,
      cacheEnabled: true,
      cacheSize: 4096,
      cacheTtlSeconds: 3600,
      failureTtlSeconds: 60,
    },
    ecs: { mode: "inherit" },
  };
}

function createRule(settings: StandardModeSettings): StandardRoutingRule {
  const id = nextId(
    "rule",
    settings.routing.rules.map((rule) => rule.id),
  );
  return {
    id,
    name: id,
    enabled: true,
    condition: { type: "suffix", values: [] },
    action: {
      type: "use_path",
      pathId: settings.paths[1]?.id ?? settings.paths[0]?.id ?? "default",
    },
    source: "manual",
  };
}

function createRuleDataSource(
  role: StandardRuleDataSettings[RuleDataRoleKey],
  type: StandardRuleDataSource["type"],
): StandardRuleDataSource {
  const id = nextId(
    type,
    role.sources.map((source) => source.id),
  );
  const base = { id, name: id, enabled: true };
  if (type === "manual") return { ...base, type, rules: [] };
  if (type === "local_file") return { ...base, type, path: "" };
  if (type === "subscription") {
    return {
      ...base,
      type,
      url: "",
      updateIntervalHours: 24,
      maxAgeHours: 72,
    };
  }
  return { ...base, type: "native_dat", path: "", selectors: [] };
}

export default function StandardRoutingPage() {
  const storeSettings = useAppStore((s) => s.standardSettings);
  const buildInfo = useAppStore((s) => s.buildInfo);
  const standardLastGenerated = useAppStore((s) => s.standardLastGenerated);
  const saveStandardSettings = useAppStore((s) => s.saveStandardSettings);
  const isConfigSaving = useAppStore((s) => s.isConfigSaving);
  const isApplying = useAppStore((s) => s.isApplying);
  const { t, formatNumber } = useI18n();
  const [draftSettings, setDraftSettings] =
    useState<StandardModeSettings | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [runtimeState, setRuntimeState] = useState<RuleDataRuntimeState>({
    providers: {},
    subscriptions: {},
  });
  const [runtimeLoading, setRuntimeLoading] = useState(false);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);
  const [runtimeObservedAtMs, setRuntimeObservedAtMs] = useState(0);
  const [refreshingSource, setRefreshingSource] = useState<string | null>(null);
  const settings = draftSettings ?? storeSettings;
  const capabilities = useMemo(
    () => standardRoutingCapabilityMap(buildInfo),
    [buildInfo],
  );
  const validationIssues = useMemo(
    () => validateStandardRoutingSettings(settings, buildInfo),
    [settings, buildInfo],
  );
  const isBusy = isConfigSaving || isApplying;
  const canSave = validationIssues.length === 0 && !isBusy;
  const enabledRules = settings.routing.rules.filter((rule) => rule.enabled);
  const ruleDataRuntimeTags = useMemo(
    () => standardLastGenerated?.tagMap.ruleData ?? {},
    [standardLastGenerated],
  );
  const ruleDataSourceTags = useMemo(
    () => standardLastGenerated?.tagMap.ruleDataSources ?? {},
    [standardLastGenerated],
  );

  const loadRuleDataRuntime = useCallback(async () => {
    const providerEntries = Object.entries(ruleDataRuntimeTags);
    const subscriptionEntries = Object.entries(ruleDataSourceTags);
    if (providerEntries.length === 0 && subscriptionEntries.length === 0) {
      setRuntimeState({ providers: {}, subscriptions: {} });
      setRuntimeError(null);
      return;
    }
    setRuntimeLoading(true);
    setRuntimeError(null);
    const failures: Error[] = [];
    const [providers, subscriptions] = await Promise.all([
      Promise.all(
        providerEntries.map(async ([role, tag]) => [
          role,
          await fetchProviderStatus(tag).catch((error) => {
            failures.push(
              error instanceof Error ? error : new Error(String(error)),
            );
            return null;
          }),
        ] as const),
      ),
      Promise.all(
        subscriptionEntries.map(async ([sourceKey, tags]) => [
          sourceKey,
          await fetchDownloadStatus(tags.download).catch((error) => {
            failures.push(
              error instanceof Error ? error : new Error(String(error)),
            );
            return null;
          }),
        ] as const),
      ),
    ]);
    setRuntimeState({
      providers: Object.fromEntries(providers),
      subscriptions: Object.fromEntries(subscriptions),
    });
    setRuntimeObservedAtMs(Date.now());
    setRuntimeError(failures[0]?.message ?? null);
    setRuntimeLoading(false);
  }, [ruleDataRuntimeTags, ruleDataSourceTags]);

  useEffect(() => {
    const timer = window.setTimeout(() => void loadRuleDataRuntime(), 0);
    return () => window.clearTimeout(timer);
  }, [loadRuleDataRuntime]);

  const refreshRuleDataSource = async (sourceKey: string) => {
    const tags = ruleDataSourceTags[sourceKey];
    if (!tags) return;
    setRefreshingSource(sourceKey);
    setRuntimeError(null);
    try {
      const result = await runCronJob(tags.cron, tags.job);
      if (!result.ok) {
        throw new Error(
          result.last_error || t(WEBUI.standardRouting.sourceRefreshFailed),
        );
      }
      await loadRuleDataRuntime();
    } catch (error) {
      setRuntimeError(error instanceof Error ? error.message : String(error));
    } finally {
      setRefreshingSource(null);
    }
  };

  const setSettings = (nextSettings: StandardModeSettings) => {
    setSaveError(null);
    setDraftSettings(nextSettings);
  };

  const setPartial = (patch: Partial<StandardModeSettings>) => {
    setSettings({ ...settings, ...patch });
  };

  const setRouting = (patch: Partial<StandardModeSettings["routing"]>) => {
    setPartial({ routing: { ...settings.routing, ...patch } });
  };

  const updatePath = (
    pathId: string,
    patch: Partial<StandardResolutionPath>,
  ) => {
    setPartial({
      paths: settings.paths.map((path) =>
        path.id === pathId ? { ...path, ...patch } : path,
      ),
    });
  };

  const removePath = (pathId: string) => {
    if (
      pathId === settings.paths[0]?.id ||
      selectStandardPathReferences(settings, pathId).length > 0
    ) {
      return;
    }
    setPartial({
      paths: settings.paths.filter((path) => path.id !== pathId),
    });
  };

  const updateRule = (ruleId: string, patch: Partial<StandardRoutingRule>) => {
    setRouting({
      rules: settings.routing.rules.map((rule) =>
        rule.id === ruleId ? { ...rule, ...patch } : rule,
      ),
    });
  };

  const removeRule = (ruleId: string) => {
    setRouting({
      rules: settings.routing.rules.filter((rule) => rule.id !== ruleId),
    });
  };

  const handleSave = async () => {
    const nextSettings = normalizeStandardRoutingSettings(settings);
    const issues = validateStandardRoutingSettings(nextSettings, buildInfo);
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
      <AppHeader title={t(WEBUI.standardRouting.title)} />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-6xl space-y-6">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <h1 className="text-xl font-semibold tracking-tight">
                {t(WEBUI.standardRouting.title)}
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                {t(WEBUI.standardRouting.description)}
              </p>
            </div>
            <Button onClick={handleSave} disabled={!canSave}>
              {isBusy ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Save className="size-4" />
              )}
              {isBusy
                ? t(WEBUI.standardRouting.savingApplying)
                : t(WEBUI.standardRouting.saveApply)}
            </Button>
          </div>

          {validationIssues.length > 0 || saveError ? (
            <ValidationPanel issues={validationIssues} saveError={saveError} />
          ) : null}

          <div className="grid gap-3 sm:grid-cols-3">
            <MetricCard
              label={t(WEBUI.standardRouting.pathCount)}
              value={formatNumber(settings.paths.length)}
            />
            <MetricCard
              label={t(WEBUI.standardRouting.ruleCount)}
              value={formatNumber(settings.routing.rules.length)}
            />
            <MetricCard
              label={t(WEBUI.standardRouting.enabledRuleCount)}
              value={formatNumber(enabledRules.length)}
            />
          </div>

          <Card>
            <CardHeader className="flex flex-row items-center justify-between space-y-0">
              <CardTitle className="flex items-center gap-2 text-base">
                <GitBranch className="size-4" />
                {t(WEBUI.standardRouting.overviewTitle)}
              </CardTitle>
              {!capabilities.sequence ? (
                <Badge variant="destructive">
                  {t(WEBUI.standardRouting.unsupportedRouting)}
                </Badge>
              ) : null}
            </CardHeader>
            <CardContent>
              <Label className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal">
                {t(WEBUI.standardRouting.enabled)}
                <Switch
                  checked={settings.routing.enabled}
                  onCheckedChange={(checked) =>
                    setRouting({ enabled: checked })
                  }
                />
              </Label>
            </CardContent>
          </Card>

          <SmartRoutingEditor
            settings={settings}
            onChange={(smartRouting) => setPartial({ smartRouting })}
          />

          <RuleDataEditor
            ruleData={settings.ruleData}
            runtimeState={runtimeState}
            runtimeLoading={runtimeLoading}
            runtimeError={runtimeError}
            runtimeObservedAtMs={runtimeObservedAtMs}
            runtimeTags={ruleDataRuntimeTags}
            sourceTags={ruleDataSourceTags}
            refreshingSource={refreshingSource}
            onRefreshRuntime={() => void loadRuleDataRuntime()}
            onRefreshSource={(sourceKey) => void refreshRuleDataSource(sourceKey)}
            onChange={(ruleData) => setPartial({ ruleData })}
          />

          <Card>
            <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
              <div>
                <CardTitle className="flex items-center gap-2 text-base">
                  <Route className="size-4" />
                  {t(WEBUI.standardRouting.pathsTitle)}
                </CardTitle>
                <p className="mt-1 text-sm text-muted-foreground">
                  {t(WEBUI.standardRouting.pathsDescription)}
                </p>
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() =>
                  setPartial({
                    paths: [...settings.paths, createPath(settings.paths)],
                  })
                }
              >
                <Plus className="size-4" />
                {t(WEBUI.standardRouting.addPath)}
              </Button>
            </CardHeader>
            <CardContent className="space-y-3">
              {settings.paths.map((path, index) => {
                const references = selectStandardPathReferences(
                  settings,
                  path.id,
                );
                return (
                  <PathEditor
                    key={path.id}
                    path={path}
                    isDefault={index === 0}
                    canRemove={index > 0 && references.length === 0}
                    references={references}
                    settings={settings}
                    onChange={(patch) => updatePath(path.id, patch)}
                    onRemove={() => removePath(path.id)}
                  />
                );
              })}
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
              <div>
                <CardTitle className="text-base">
                  {t(WEBUI.standardRouting.rulesTitle)}
                </CardTitle>
                <p className="mt-1 text-sm text-muted-foreground">
                  {t(WEBUI.standardRouting.rulesDescription)}
                </p>
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() =>
                  setRouting({
                    rules: [...settings.routing.rules, createRule(settings)],
                  })
                }
              >
                <Plus className="size-4" />
                {t(WEBUI.standardRouting.addRule)}
              </Button>
            </CardHeader>
            <CardContent className="space-y-3">
              {settings.routing.rules.length === 0 ? (
                <div className="rounded-lg border border-dashed p-6 text-sm text-muted-foreground">
                  {t(WEBUI.standardRouting.rulesEmpty)}
                </div>
              ) : (
                settings.routing.rules.map((rule) => (
                  <RuleEditor
                    key={rule.id}
                    rule={rule}
                    settings={settings}
                    capabilities={capabilities}
                    onChange={(patch) => updateRule(rule.id, patch)}
                    onRemove={() => removeRule(rule.id)}
                  />
                ))
              )}
            </CardContent>
          </Card>
        </div>
      </main>
    </>
  );
}

function SmartRoutingEditor({
  settings,
  onChange,
}: {
  settings: StandardModeSettings;
  onChange: (value: StandardModeSettings["smartRouting"]) => void;
}) {
  const { t } = useI18n();
  const smart = settings.smartRouting;
  const patch = (next: Partial<typeof smart>) =>
    onChange({ ...smart, ...next });
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <ShieldCheck className="size-4" />
          {t(WEBUI.standardRouting.smartTitle)}
        </CardTitle>
        <p className="text-sm text-muted-foreground">
          {t(WEBUI.standardRouting.smartDescription)}
        </p>
      </CardHeader>
      <CardContent className="space-y-4">
        <Label className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal">
          {t(WEBUI.standardRouting.smartEnabled)}
          <Switch
            checked={smart.enabled}
            onCheckedChange={(enabled) => patch({ enabled })}
          />
        </Label>
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          <div className="space-y-2">
            <Label>{t(WEBUI.standardRouting.domesticPath)}</Label>
            <Select
              value={smart.domesticPathId ?? ""}
              onValueChange={(domesticPathId) => patch({ domesticPathId })}
            >
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                {settings.paths.map((path) => (
                  <SelectItem key={path.id} value={path.id}>{path.name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label>{t(WEBUI.standardRouting.remotePath)}</Label>
            <Select
              value={smart.remotePathId ?? ""}
              onValueChange={(remotePathId) => patch({ remotePathId })}
            >
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                {settings.paths.map((path) => (
                  <SelectItem key={path.id} value={path.id}>{path.name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label>{t(WEBUI.standardRouting.unknownMode)}</Label>
            <Select
              value={smart.unknownMode}
              onValueChange={(unknownMode) =>
                patch({
                  unknownMode: unknownMode as typeof smart.unknownMode,
                  ...(unknownMode === "strict_remote"
                    ? { privacyFallbackToDomestic: false }
                    : {}),
                })
              }
            >
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="compatibility_first">{t(WEBUI.standardRouting.unknownCompatibility)}</SelectItem>
                <SelectItem value="privacy_first">{t(WEBUI.standardRouting.unknownPrivacy)}</SelectItem>
                <SelectItem value="strict_remote">{t(WEBUI.standardRouting.unknownStrictRemote)}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label>{t(WEBUI.standardRouting.fallbackThreshold)}</Label>
            <Input
              type="number"
              min={1}
              value={smart.fallbackThresholdMs}
              onChange={(event) =>
                patch({ fallbackThresholdMs: Number(event.target.value) })
              }
            />
          </div>
        </div>
        <Label className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal">
          {t(WEBUI.standardRouting.privacyDomesticFallback)}
          <Switch
            checked={smart.privacyFallbackToDomestic}
            disabled={smart.unknownMode === "strict_remote"}
            onCheckedChange={(privacyFallbackToDomestic) =>
              patch({ privacyFallbackToDomestic })
            }
          />
        </Label>
        <div className="space-y-2">
          <Label>{t(WEBUI.standardRouting.responsePolicyTitle)}</Label>
          <div className="grid gap-2 md:grid-cols-2 xl:grid-cols-4">
            {(Object.keys(smartResponsePolicyLabelKeys) as Array<
              keyof typeof smart.responsePolicy
            >).map((key) => (
              <Label
                key={key}
                className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal"
              >
                {t(smartResponsePolicyLabelKeys[key])}
                <Switch
                  checked={smart.responsePolicy[key]}
                  onCheckedChange={(enabled) =>
                    patch({
                      responsePolicy: {
                        ...smart.responsePolicy,
                        [key]: enabled,
                      },
                    })
                  }
                />
              </Label>
            ))}
          </div>
        </div>
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/5 p-3 text-sm text-muted-foreground">
          {t(WEBUI.standardRouting.leakBoundary)}
        </div>
      </CardContent>
    </Card>
  );
}

function RuleDataEditor({
  ruleData,
  runtimeState,
  runtimeLoading,
  runtimeError,
  runtimeObservedAtMs,
  runtimeTags,
  sourceTags,
  refreshingSource,
  onRefreshRuntime,
  onRefreshSource,
  onChange,
}: {
  ruleData: StandardRuleDataSettings;
  runtimeState: RuleDataRuntimeState;
  runtimeLoading: boolean;
  runtimeError: string | null;
  runtimeObservedAtMs: number;
  runtimeTags: Record<string, string>;
  sourceTags: Record<string, { download: string; cron: string; job: string }>;
  refreshingSource: string | null;
  onRefreshRuntime: () => void;
  onRefreshSource: (sourceKey: string) => void;
  onChange: (value: StandardRuleDataSettings) => void;
}) {
  const { t, formatNumber } = useI18n();
  const updateRole = (
    roleKey: RuleDataRoleKey,
    sources: StandardRuleDataSource[],
  ) => onChange({ ...ruleData, [roleKey]: { sources } });
  return (
    <Card>
      <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
        <div>
          <CardTitle className="flex items-center gap-2 text-base">
            <Database className="size-4" />
            {t(WEBUI.standardRouting.ruleDataTitle)}
          </CardTitle>
          <p className="mt-1 text-sm text-muted-foreground">
            {t(WEBUI.standardRouting.ruleDataDescription)}
          </p>
        </div>
        <Button type="button" variant="outline" size="sm" onClick={onRefreshRuntime} disabled={runtimeLoading}>
          <RefreshCw className={`size-4 ${runtimeLoading ? "animate-spin" : ""}`} />
          {t(WEBUI.common.refresh)}
        </Button>
      </CardHeader>
      <CardContent className="space-y-4">
        {runtimeError ? (
          <div className="rounded-lg border border-destructive/40 bg-destructive/5 p-3 text-sm text-destructive">
            {runtimeError}
          </div>
        ) : null}
        {ruleDataRoles.map((roleKey) => {
          const role = ruleData[roleKey];
          const runtimeKey = ruleDataRuntimeKeys[roleKey];
          const provider = runtimeState.providers[runtimeKey];
          const providerTag = runtimeTags[runtimeKey];
          const ruleCount = provider?.rule_stats?.total_rules;
          return (
            <div key={roleKey} className="rounded-lg border p-4">
              <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
                <div className="flex flex-wrap items-center gap-2">
                  <div>
                    <div className="font-medium">{t(ruleDataRoleLabelKeys[roleKey])}</div>
                    <div className="font-mono text-xs text-muted-foreground">{runtimeKey}</div>
                  </div>
                  {!providerTag ? (
                    <Badge variant="outline">{t(WEBUI.standardRouting.sourceNotApplied)}</Badge>
                  ) : provider?.last_error ? (
                    <Badge variant="destructive">{t(WEBUI.standardRouting.sourceLoadFailed)}</Badge>
                  ) : provider ? (
                    <Badge variant="secondary">
                      {ruleCount == null
                        ? t(WEBUI.standardRouting.sourceLoaded)
                        : t(WEBUI.standardRouting.sourceRuleCount, {
                            count: formatNumber(ruleCount),
                          })}
                    </Badge>
                  ) : (
                    <Badge variant="outline">{t(WEBUI.standardRouting.sourceStatusUnavailable)}</Badge>
                  )}
                </div>
                <div className="flex flex-wrap gap-2">
                  {(["manual", "local_file", "subscription", "native_dat"] as const).map((type) => (
                    <Button
                      key={type}
                      type="button"
                      size="sm"
                      variant="outline"
                      onClick={() =>
                        updateRole(roleKey, [
                          ...role.sources,
                          createRuleDataSource(role, type),
                        ])
                      }
                    >
                      <Plus className="size-3" /> {t(ruleDataSourceTypeLabelKeys[type])}
                    </Button>
                  ))}
                </div>
              </div>
              {role.sources.length === 0 ? (
                <p className="text-sm text-muted-foreground">
                  {t(WEBUI.standardRouting.ruleDataEmpty)}
                </p>
              ) : (
                <div className="space-y-3">
                  {role.sources.map((source, index) => (
                    <RuleDataSourceEditor
                      key={`${source.id}-${index}`}
                      source={source}
                      sourceKey={`${runtimeKey}:${source.id}`}
                      download={runtimeState.subscriptions[`${runtimeKey}:${source.id}`]}
                      observedAtMs={runtimeObservedAtMs}
                      hasRuntimeTags={Boolean(sourceTags[`${runtimeKey}:${source.id}`])}
                      refreshing={refreshingSource === `${runtimeKey}:${source.id}`}
                      onRefresh={() => onRefreshSource(`${runtimeKey}:${source.id}`)}
                      onChange={(next) =>
                        updateRole(
                          roleKey,
                          role.sources.map((item, itemIndex) =>
                            itemIndex === index ? next : item,
                          ),
                        )
                      }
                      onRemove={() =>
                        updateRole(
                          roleKey,
                          role.sources.filter((_, itemIndex) => itemIndex !== index),
                        )
                      }
                    />
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </CardContent>
    </Card>
  );
}

function RuleDataSourceEditor({
  source,
  sourceKey,
  download,
  observedAtMs,
  hasRuntimeTags,
  refreshing,
  onRefresh,
  onChange,
  onRemove,
}: {
  source: StandardRuleDataSource;
  sourceKey: string;
  download?: DownloadStatusResponse | null;
  observedAtMs: number;
  hasRuntimeTags: boolean;
  refreshing: boolean;
  onRefresh: () => void;
  onChange: (value: StandardRuleDataSource) => void;
  onRemove: () => void;
}) {
  const { t } = useI18n();
  const update = (patch: Partial<StandardRuleDataSource>) =>
    onChange({ ...source, ...patch } as StandardRuleDataSource);
  const item = download?.items[0];
  const stale =
    source.type === "subscription" &&
    item?.file.modified_at_ms != null &&
    observedAtMs - item.file.modified_at_ms >
      source.maxAgeHours * 60 * 60 * 1000;
  return (
    <div className="rounded-md border bg-muted/10 p-3">
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <span className="font-mono text-xs text-muted-foreground">{sourceKey}</span>
        {source.type === "subscription" ? (
          !hasRuntimeTags ? (
            <Badge variant="outline">{t(WEBUI.standardRouting.sourceNotApplied)}</Badge>
          ) : item?.last_error ? (
            <Badge variant="destructive">{t(WEBUI.standardRouting.sourceDownloadFailed)}</Badge>
          ) : item && !item.file.exists ? (
            <Badge variant="destructive">{t(WEBUI.standardRouting.sourceFileMissing)}</Badge>
          ) : stale ? (
            <Badge variant="outline">{t(WEBUI.standardRouting.sourceFileStale)}</Badge>
          ) : item?.file.exists ? (
            <Badge variant="secondary">{t(WEBUI.standardRouting.sourceReady)}</Badge>
          ) : (
            <Badge variant="outline">{t(WEBUI.standardRouting.sourceStatusUnavailable)}</Badge>
          )
        ) : null}
        {source.type === "subscription" && hasRuntimeTags ? (
          <Button type="button" variant="ghost" size="sm" onClick={onRefresh} disabled={refreshing}>
            <RefreshCw className={`size-3 ${refreshing ? "animate-spin" : ""}`} />
            {t(WEBUI.standardRouting.sourceRefresh)}
          </Button>
        ) : null}
        {item?.last_error ? (
          <span className="text-xs text-destructive">{item.last_error}</span>
        ) : null}
      </div>
      <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
        <Label className="flex min-h-10 items-center gap-2 text-sm font-normal">
          <Switch checked={source.enabled} onCheckedChange={(enabled) => update({ enabled })} />
          {t(ruleDataSourceTypeLabelKeys[source.type])}
        </Label>
        <div className="space-y-2">
          <Label>{t(WEBUI.standardRouting.sourceId)}</Label>
          <Input value={source.id} onChange={(event) => update({ id: event.target.value })} placeholder="source_id" />
        </div>
        <div className="space-y-2">
          <Label>{t(WEBUI.standardRouting.sourceName)}</Label>
          <Input value={source.name} onChange={(event) => update({ name: event.target.value })} />
        </div>
        <Button type="button" variant="ghost" size="sm" onClick={onRemove}>
          <Trash2 className="size-4" /> {t(WEBUI.standardRouting.removeRule)}
        </Button>
        {source.type === "manual" ? (
          <Textarea
            className="min-h-24 font-mono text-sm md:col-span-2 xl:col-span-4"
            value={source.rules.join("\n")}
            placeholder={t(WEBUI.standardRouting.sourceRules)}
            onChange={(event) => update({ rules: lines(event.target.value) })}
          />
        ) : null}
        {source.type === "local_file" || source.type === "native_dat" ? (
          <Input
            className="md:col-span-2"
            value={source.path}
            placeholder={t(WEBUI.standardRouting.sourcePath)}
            onChange={(event) => update({ path: event.target.value })}
          />
        ) : null}
        {source.type === "native_dat" ? (
          <Textarea
            className="min-h-20 font-mono text-sm md:col-span-2"
            value={source.selectors.join("\n")}
            placeholder={t(WEBUI.standardRouting.sourceSelectors)}
            onChange={(event) => update({ selectors: lines(event.target.value) })}
          />
        ) : null}
        {source.type === "subscription" ? (
          <>
            <div className="space-y-2 md:col-span-2">
              <Label>{t(WEBUI.standardRouting.sourceUrl)}</Label>
              <Input value={source.url} placeholder="https://…" onChange={(event) => update({ url: event.target.value })} />
            </div>
            <div className="space-y-2">
              <Label>{t(WEBUI.standardRouting.sourceUpdateInterval)}</Label>
              <Input type="number" min={1} value={source.updateIntervalHours} onChange={(event) => update({ updateIntervalHours: Number(event.target.value) })} />
            </div>
            <div className="space-y-2">
              <Label>{t(WEBUI.standardRouting.sourceMaxAge)}</Label>
              <Input type="number" min={1} value={source.maxAgeHours} onChange={(event) => update({ maxAgeHours: Number(event.target.value) })} />
            </div>
          </>
        ) : null}
      </div>
    </div>
  );
}

function MetricCard({ label, value }: { label: string; value: string }) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-medium text-muted-foreground">
          {label}
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-semibold">{value}</div>
      </CardContent>
    </Card>
  );
}

function PathEditor({
  path,
  isDefault,
  canRemove,
  references,
  settings,
  onChange,
  onRemove,
}: {
  path: StandardResolutionPath;
  isDefault: boolean;
  canRemove: boolean;
  references: StandardEntityReference[];
  settings: StandardModeSettings;
  onChange: (patch: Partial<StandardResolutionPath>) => void;
  onRemove: () => void;
}) {
  const { t } = useI18n();
  return (
    <div
      id={`path-${path.id}`}
      className="scroll-mt-6 rounded-lg border bg-card/40 p-4"
    >
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <span className="font-medium">{path.name || path.id}</span>
          {isDefault ? (
            <Badge variant="secondary">{t(WEBUI.common.defaultValue)}</Badge>
          ) : null}
        </div>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={!canRemove}
          onClick={onRemove}
        >
          <Trash2 className="size-4" />
          {t(WEBUI.standardRouting.removePath)}
        </Button>
      </div>
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <div className="space-y-2">
          <Label htmlFor={`${path.id}-name`}>
            {t(WEBUI.standardRouting.pathName)}
          </Label>
          <Input
            id={`${path.id}-name`}
            value={path.name}
            onChange={(event) => onChange({ name: event.target.value })}
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor={`${path.id}-group`}>
            {t(WEBUI.standardRouting.pathUpstreamGroup)}
          </Label>
          <Select
            value={path.upstreamGroupId}
            onValueChange={(value) => onChange({ upstreamGroupId: value })}
          >
            <SelectTrigger id={`${path.id}-group`} className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {settings.upstreamGroups.map((group) => (
                <SelectItem key={group.id} value={group.id}>
                  {group.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <PolicySelect
          id={`${path.id}-filtering`}
          label={t(WEBUI.standardRouting.pathFiltering)}
          value={path.filtering}
          onChange={(value) => onChange({ filtering: value })}
        />
        <PolicySelect
          id={`${path.id}-cache`}
          label={t(WEBUI.standardRouting.pathCache)}
          value={path.cache}
          onChange={(value) => onChange({ cache: value })}
        />
        <PolicySelect
          id={`${path.id}-query-log`}
          label={t(WEBUI.standardRouting.pathQueryLog)}
          value={path.queryLog}
          onChange={(value) => onChange({ queryLog: value })}
        />
        <div className="space-y-2 md:col-span-2 xl:col-span-3">
          <Label htmlFor={`${path.id}-description`}>
            {t(WEBUI.standardRouting.pathDescription)}
          </Label>
          <Input
            id={`${path.id}-description`}
            value={path.description ?? ""}
            onChange={(event) => onChange({ description: event.target.value })}
          />
        </div>
      </div>
      <div className="mt-4 grid gap-4 rounded-lg border bg-muted/10 p-3 md:grid-cols-2 xl:grid-cols-4">
        <div className="space-y-2">
          <Label>{t(WEBUI.standardRouting.pathDualStack)}</Label>
          <Select
            value={path.dualStack}
            onValueChange={(dualStack) =>
              onChange({ dualStack: dualStack as StandardResolutionPath["dualStack"] })
            }
          >
            <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              {(["inherit", "disabled", "prefer_ipv4", "prefer_ipv6", "ipv4_only", "ipv6_only"] as const).map((mode) => (
                <SelectItem key={mode} value={mode}>{mode}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-2">
          <Label>{t(WEBUI.standardRouting.pathEcs)}</Label>
          <Select
            value={path.ecs.mode}
            onValueChange={(mode) => {
              if (mode === "client_subnet") onChange({ ecs: { mode, mask4: 24, mask6: 48 } });
              else if (mode === "preset") onChange({ ecs: { mode, address: "", mask4: 24, mask6: 48 } });
              else onChange({ ecs: { mode } as StandardResolutionPath["ecs"] });
            }}
          >
            <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              {(["inherit", "remove", "preserve_client", "client_subnet", "preset"] as const).map((mode) => (
                <SelectItem key={mode} value={mode}>{mode}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        {path.ecs.mode === "preset" ? (
          <div className="space-y-2">
            <Label>{t(WEBUI.standardRouting.ecsPreset)}</Label>
            <Input value={path.ecs.address} onChange={(event) => onChange({ ecs: { ...path.ecs, address: event.target.value } as StandardResolutionPath["ecs"] })} />
          </div>
        ) : null}
        {path.ecs.mode === "client_subnet" || path.ecs.mode === "preset" ? (
          <div className="grid grid-cols-2 gap-2">
            <div className="space-y-2"><Label>IPv4</Label><Input type="number" min={0} max={32} value={path.ecs.mask4} onChange={(event) => onChange({ ecs: { ...path.ecs, mask4: Number(event.target.value) } as StandardResolutionPath["ecs"] })} /></div>
            <div className="space-y-2"><Label>IPv6</Label><Input type="number" min={0} max={128} value={path.ecs.mask6} onChange={(event) => onChange({ ecs: { ...path.ecs, mask6: Number(event.target.value) } as StandardResolutionPath["ecs"] })} /></div>
          </div>
        ) : null}
        <Label className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal">
          {t(WEBUI.standardRouting.pathIpSelection)}
          <Switch
            checked={path.ipSelection.enabled}
            onCheckedChange={(enabled) => onChange({ ipSelection: { ...path.ipSelection, enabled } })}
          />
        </Label>
        {path.ipSelection.enabled ? (
          <>
            <div className="space-y-2">
              <Label>{t(WEBUI.standardRouting.ipSelectionMode)}</Label>
              <Select value={path.ipSelection.selectionMode} onValueChange={(selectionMode) => onChange({ ipSelection: { ...path.ipSelection, selectionMode: selectionMode as typeof path.ipSelection.selectionMode } })}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="first_success">first_success</SelectItem>
                  <SelectItem value="best_within_budget">best_within_budget</SelectItem>
                  <SelectItem value="background">background</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label>{t(WEBUI.standardRouting.dnssecPolicy)}</Label>
              <Select value={path.ipSelection.dnssecPolicy} onValueChange={(dnssecPolicy) => onChange({ ipSelection: { ...path.ipSelection, dnssecPolicy: dnssecPolicy as typeof path.ipSelection.dnssecPolicy } })}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent><SelectItem value="reorder_only">reorder_only</SelectItem><SelectItem value="skip">skip</SelectItem></SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label>{t(WEBUI.standardRouting.probeMethods)}</Label>
              <Input value={path.ipSelection.probeMethods.join(",")} onChange={(event) => onChange({ ipSelection: { ...path.ipSelection, probeMethods: event.target.value.split(",").map((value) => value.trim()).filter(Boolean) } })} />
            </div>
          </>
        ) : null}
      </div>
      <div className="mt-4 rounded-lg border bg-muted/20 p-3 text-sm">
        <div className="font-medium">
          {t(WEBUI.standardRouting.pathReferences)}
        </div>
        {references.length > 0 ? (
          <div className="mt-2 flex flex-wrap gap-2">
            {references.map((reference) => (
              <Badge
                key={`${reference.kind}-${reference.id}`}
                variant={reference.enabled ? "secondary" : "outline"}
                asChild
              >
                <Link href={reference.href}>
                  {t(referenceKindLabelKeys[reference.kind])}: {reference.name}
                </Link>
              </Badge>
            ))}
          </div>
        ) : (
          <p className="mt-1 text-muted-foreground">
            {t(WEBUI.standardRouting.pathNoReferences)}
          </p>
        )}
      </div>
    </div>
  );
}

function PolicySelect({
  id,
  label,
  value,
  onChange,
}: {
  id: string;
  label: string;
  value: PathPolicy;
  onChange: (value: PathPolicy) => void;
}) {
  const { t } = useI18n();
  return (
    <div className="space-y-2">
      <Label htmlFor={id}>{label}</Label>
      <Select
        value={value}
        onValueChange={(next) => onChange(next as PathPolicy)}
      >
        <SelectTrigger id={id} className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {(Object.keys(policyLabelKeys) as PathPolicy[]).map((policy) => (
            <SelectItem key={policy} value={policy}>
              {t(policyLabelKeys[policy])}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}

function RuleEditor({
  rule,
  settings,
  capabilities,
  onChange,
  onRemove,
}: {
  rule: StandardRoutingRule;
  settings: StandardModeSettings;
  capabilities: ReturnType<typeof standardRoutingCapabilityMap>;
  onChange: (patch: Partial<StandardRoutingRule>) => void;
  onRemove: () => void;
}) {
  const { t } = useI18n();
  const conditionType = isSupportedCondition(rule.condition.type)
    ? rule.condition.type
    : "suffix";
  const values = "values" in rule.condition ? rule.condition.values : [];
  const matcherSupported =
    conditionType === "client_cidr"
      ? capabilities.clientIp
      : conditionType === "qtype"
        ? capabilities.qtype
        : capabilities.qname;
  const targetPathId =
    rule.action.type === "use_path"
      ? rule.action.pathId
      : (settings.paths[0]?.id ?? "default");

  return (
    <div
      id={`rule-${rule.id}`}
      className="scroll-mt-6 rounded-lg border bg-card/40 p-4"
    >
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <Label className="text-sm font-normal">
          <Switch
            checked={rule.enabled}
            onCheckedChange={(checked) => onChange({ enabled: checked })}
          />
          {t(WEBUI.standardRouting.ruleEnabled)}
        </Label>
        <div className="flex items-center gap-2">
          {!matcherSupported ? (
            <Badge variant="destructive">
              {t(WEBUI.standardRouting.unsupportedMatcher)}
            </Badge>
          ) : null}
          <Button type="button" variant="ghost" size="sm" onClick={onRemove}>
            <Trash2 className="size-4" />
            {t(WEBUI.standardRouting.removeRule)}
          </Button>
        </div>
      </div>
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <div className="space-y-2">
          <Label htmlFor={`${rule.id}-name`}>
            {t(WEBUI.standardRouting.ruleName)}
          </Label>
          <Input
            id={`${rule.id}-name`}
            value={rule.name}
            onChange={(event) => onChange({ name: event.target.value })}
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor={`${rule.id}-condition`}>
            {t(WEBUI.standardRouting.ruleCondition)}
          </Label>
          <Select
            value={conditionType}
            onValueChange={(value) =>
              onChange({
                condition: {
                  type: value as RoutingConditionType,
                  values: [],
                },
              })
            }
          >
            <SelectTrigger id={`${rule.id}-condition`} className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(Object.keys(conditionLabelKeys) as RoutingConditionType[]).map(
                (type) => (
                  <SelectItem key={type} value={type}>
                    {t(conditionLabelKeys[type])}
                  </SelectItem>
                ),
              )}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-2">
          <Label htmlFor={`${rule.id}-path`}>
            {t(WEBUI.standardRouting.ruleTargetPath)}
          </Label>
          <Select
            value={targetPathId}
            onValueChange={(value) =>
              onChange({ action: { type: "use_path", pathId: value } })
            }
          >
            <SelectTrigger id={`${rule.id}-path`} className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {settings.paths.map((path) => (
                <SelectItem key={path.id} value={path.id}>
                  {path.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-2">
          <Label htmlFor={`${rule.id}-note`}>
            {t(WEBUI.standardRouting.ruleNote)}
          </Label>
          <Input
            id={`${rule.id}-note`}
            value={rule.note ?? ""}
            onChange={(event) => onChange({ note: event.target.value })}
          />
        </div>
        <div className="space-y-2 md:col-span-2 xl:col-span-4">
          <Label htmlFor={`${rule.id}-values`}>
            {t(WEBUI.standardRouting.ruleValues)}
          </Label>
          <Textarea
            id={`${rule.id}-values`}
            className="min-h-24 font-mono text-sm"
            value={values.join("\n")}
            placeholder={conditionPlaceholders[conditionType]}
            onChange={(event) =>
              onChange({
                condition: {
                  type: conditionType,
                  values: lines(event.target.value),
                },
              })
            }
          />
        </div>
      </div>
    </div>
  );
}

function ValidationPanel({
  issues,
  saveError,
}: {
  issues: StandardRoutingValidationIssue[];
  saveError: string | null;
}) {
  const { t } = useI18n();
  return (
    <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm">
      <div className="font-medium">
        {t(WEBUI.standardRouting.validationTitle)}
      </div>
      <ul className="mt-2 list-disc space-y-1 pl-5 text-muted-foreground">
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
  issue: StandardRoutingValidationIssue,
  t: ReturnType<typeof useI18n>["t"],
) {
  if (issue.code === "capability_required") {
    return t(WEBUI.standardRouting.validationCapabilityRequired);
  }
  if (issue.code === "path_required") {
    return t(WEBUI.standardRouting.validationPathRequired);
  }
  if (issue.code === "path_name_required") {
    return t(WEBUI.standardRouting.validationPathNameRequired);
  }
  if (issue.code === "path_upstream_group_required") {
    return t(WEBUI.standardRouting.validationPathUpstreamRequired);
  }
  if (
    issue.code === "path_ecs_invalid" ||
    issue.code === "path_ip_selection_invalid"
  ) {
    return t(WEBUI.standardRouting.validationPathTransportInvalid);
  }
  if (issue.code === "smart_path_required") {
    return t(WEBUI.standardRouting.validationSmartPathRequired);
  }
  if (issue.code === "smart_paths_not_isolated") {
    return t(WEBUI.standardRouting.validationSmartIsolationRequired);
  }
  if (issue.code === "strict_remote_fallback_forbidden") {
    return t(WEBUI.standardRouting.validationStrictRemoteFallback);
  }
  if (issue.code === "rule_data_required") {
    return t(WEBUI.standardRouting.validationRuleDataRequired);
  }
  if (issue.code === "rule_data_source_invalid") {
    return t(WEBUI.standardRouting.validationRuleDataSourceInvalid);
  }
  if (issue.code === "path_delete_blocked") {
    return t(WEBUI.standardRouting.validationPathDeleteBlocked);
  }
  if (issue.code === "rule_name_required") {
    return t(WEBUI.standardRouting.validationRuleNameRequired);
  }
  if (issue.code === "rule_condition_required") {
    return t(WEBUI.standardRouting.validationRuleConditionRequired);
  }
  if (issue.code === "rule_action_required") {
    return t(WEBUI.standardRouting.validationRuleActionRequired);
  }
  if (issue.code === "rule_action_unsupported") {
    return t(WEBUI.standardRouting.validationRuleActionUnsupported);
  }
  if (issue.code === "rule_condition_unsupported") {
    return t(WEBUI.standardRouting.validationRuleConditionUnsupported);
  }
  return t(WEBUI.standardRouting.validationRuleMatcherUnsupported);
}

function isSupportedCondition(
  value: StandardRoutingRule["condition"]["type"],
): value is RoutingConditionType {
  return (
    value === "domain" ||
    value === "suffix" ||
    value === "keyword" ||
    value === "client_cidr" ||
    value === "qtype"
  );
}
