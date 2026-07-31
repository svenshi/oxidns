"use client";

import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import {
  ArrowRight,
  Copy,
  Loader2,
  Plus,
  Save,
  TestTube2,
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
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import {
  testUpstream,
  testUpstreamGroup,
  type UpstreamGroupTestInput,
  type UpstreamTestResult,
} from "@/lib/oxidns-api";
import { upstreamAddress } from "@/lib/standard-mode/generator";
import {
  selectDefaultUpstreamGroup,
  selectStandardCapabilityMap,
  selectStandardUpstreamGroupReferences,
} from "@/lib/standard-mode/selectors";
import type {
  StandardModeSettings,
  StandardUpstream,
  StandardUpstreamGroup,
  StandardUpstreamProtocol,
} from "@/lib/standard-mode/types";
import {
  isStandardUpstreamProtocolSupported,
  normalizeStandardDnsSettings,
  normalizeStandardUpstream,
  requiredStandardUpstreamProtocolFeatures,
  STANDARD_UPSTREAM_PROTOCOLS,
  validateStandardDnsSettings,
  type StandardDnsValidationIssue,
} from "@/lib/standard-mode/validation";
import { useAppStore } from "@/lib/store";

const protocolLabelKeys: Record<StandardUpstreamProtocol, string> = {
  auto: WEBUI.standardDns.protocolAuto,
  udp: WEBUI.standardDns.protocolUdp,
  tcp: WEBUI.standardDns.protocolTcp,
  dot: WEBUI.standardDns.protocolDot,
  doh: WEBUI.standardDns.protocolDoh,
  doh3: WEBUI.standardDns.protocolDoh3,
  doq: WEBUI.standardDns.protocolDoq,
};

function createUpstreamId(upstreams: StandardUpstream[]) {
  const used = new Set(upstreams.map((item) => item.id));
  let index = upstreams.length + 1;
  let id = `upstream_${index}`;
  while (used.has(id)) {
    index += 1;
    id = `upstream_${index}`;
  }
  return id;
}

function createUpstream(upstreams: StandardUpstream[]): StandardUpstream {
  const id = createUpstreamId(upstreams);
  return {
    id,
    name: id,
    protocol: "auto",
    address: "",
    enabled: true,
    tlsVerify: true,
  };
}

function createGroup(groups: StandardUpstreamGroup[]): StandardUpstreamGroup {
  const used = new Set(groups.map((group) => group.id));
  let index = groups.length + 1;
  let id = `group_${index}`;
  while (used.has(id)) {
    index += 1;
    id = `group_${index}`;
  }
  return {
    id,
    name: id,
    strategy: "balanced",
    upstreams: [createUpstream([])],
  };
}

function numberValue(value: string, fallback: number) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function upstreamTestInput(upstream: StandardUpstream): UpstreamGroupTestInput {
  const normalized = normalizeStandardUpstream(upstream);
  return {
    id: normalized.id,
    name: normalized.name,
    tag: normalized.id,
    addr: upstreamAddress(normalized),
    ...(normalized.bootstrap ? { bootstrap: normalized.bootstrap } : {}),
    ...(normalized.bootstrapVersion
      ? { bootstrap_version: normalized.bootstrapVersion }
      : {}),
    ...(normalized.dialAddress ? { dial_addr: normalized.dialAddress } : {}),
    ...(normalized.outbound ? { outbound: normalized.outbound } : {}),
    ...(normalized.socks5 ? { socks5: normalized.socks5 } : {}),
    ...(normalized.timeoutSeconds
      ? { timeout_seconds: normalized.timeoutSeconds }
      : {}),
    ...(normalized.idleTimeoutSeconds
      ? { idle_timeout_seconds: normalized.idleTimeoutSeconds }
      : {}),
    ...(normalized.maxConns ? { max_conns: normalized.maxConns } : {}),
    ...(normalized.minConns != null ? { min_conns: normalized.minConns } : {}),
    ...(normalized.enablePipeline ? { enable_pipeline: true } : {}),
    ...(normalized.tlsVerify === false ? { insecure_skip_verify: true } : {}),
    ...(normalized.protocol === "doh3" || normalized.enableHttp3
      ? { enable_http3: true }
      : {}),
  };
}

function failedUiTestResult(
  upstream: StandardUpstream,
  message: string,
): UpstreamTestResult {
  return {
    id: upstream.id,
    name: upstream.name || upstream.id,
    success: false,
    answers: [],
    error_code: "request_failed",
    error_message: message,
  };
}

export default function StandardDnsPage() {
  const storeSettings = useAppStore((s) => s.standardSettings);
  const buildInfo = useAppStore((s) => s.buildInfo);
  const saveStandardSettings = useAppStore((s) => s.saveStandardSettings);
  const isConfigSaving = useAppStore((s) => s.isConfigSaving);
  const isApplying = useAppStore((s) => s.isApplying);
  const { t } = useI18n();
  const capabilities = useMemo(
    () => selectStandardCapabilityMap(buildInfo),
    [buildInfo],
  );
  const [draftSettings, setDraftSettings] =
    useState<StandardModeSettings | null>(null);
  const [selectedGroupId, setSelectedGroupId] = useState(
    () => selectDefaultUpstreamGroup(storeSettings).id,
  );
  const [saveError, setSaveError] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<
    Record<string, UpstreamTestResult>
  >({});
  const [testingUpstreams, setTestingUpstreams] = useState<
    Record<string, boolean>
  >({});
  const [groupTestSummary, setGroupTestSummary] = useState<string | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const settings = draftSettings ?? storeSettings;
  const selectedGroup =
    settings.upstreamGroups.find((group) => group.id === selectedGroupId) ??
    selectDefaultUpstreamGroup(settings);
  const groupReferences = selectStandardUpstreamGroupReferences(
    settings,
    selectedGroup.id,
  );
  const validationIssues = useMemo(
    () => validateStandardDnsSettings(settings, buildInfo),
    [settings, buildInfo],
  );
  const isBusy = isConfigSaving || isApplying;
  const canSave = validationIssues.length === 0 && !isBusy;
  const testableUpstreams = selectedGroup.upstreams.filter(
    (upstream) =>
      upstream.enabled &&
      upstream.address.trim() &&
      isStandardUpstreamProtocolSupported(upstream.protocol, buildInfo),
  );
  const isGroupTesting = Object.values(testingUpstreams).some(Boolean);

  useEffect(() => {
    if (
      !settings.upstreamGroups.some((group) => group.id === selectedGroupId)
    ) {
      setSelectedGroupId(selectDefaultUpstreamGroup(settings).id);
    }
  }, [selectedGroupId, settings]);

  const setPartial = (patch: Partial<StandardModeSettings>) => {
    setSaveError(null);
    setDraftSettings((current) => ({ ...(current ?? settings), ...patch }));
  };

  const setSelectedUpstreams = (upstreams: StandardUpstream[]) => {
    setPartial({
      upstreamGroups: settings.upstreamGroups.map((group) =>
        group.id === selectedGroup.id ? { ...group, upstreams } : group,
      ),
    });
  };

  const updateSelectedGroup = (patch: Partial<StandardUpstreamGroup>) => {
    setPartial({
      upstreamGroups: settings.upstreamGroups.map((group) =>
        group.id === selectedGroup.id ? { ...group, ...patch } : group,
      ),
    });
  };

  const addGroup = () => {
    const group = createGroup(settings.upstreamGroups);
    setPartial({ upstreamGroups: [...settings.upstreamGroups, group] });
    setSelectedGroupId(group.id);
  };

  const copySelectedGroup = () => {
    const group = createGroup(settings.upstreamGroups);
    const copy: StandardUpstreamGroup = {
      ...selectedGroup,
      id: group.id,
      name: `${selectedGroup.name} copy`,
      isDefault: false,
      upstreams: selectedGroup.upstreams.map((upstream) => ({ ...upstream })),
    };
    setPartial({ upstreamGroups: [...settings.upstreamGroups, copy] });
    setSelectedGroupId(copy.id);
  };

  const removeSelectedGroup = () => {
    if (
      settings.upstreamGroups.length <= 1 ||
      selectedGroup.isDefault ||
      groupReferences.length > 0
    ) {
      return;
    }
    const remaining = settings.upstreamGroups.filter(
      (group) => group.id !== selectedGroup.id,
    );
    setPartial({ upstreamGroups: remaining });
    setSelectedGroupId(
      selectDefaultUpstreamGroup({ ...settings, upstreamGroups: remaining }).id,
    );
  };

  const setDefaultGroup = () => {
    setPartial({
      upstreamGroups: settings.upstreamGroups.map((group) => ({
        ...group,
        isDefault: group.id === selectedGroup.id,
      })),
    });
  };

  const updateUpstream = (
    upstreamId: string,
    patch: Partial<StandardUpstream>,
  ) => {
    setSelectedUpstreams(
      selectedGroup.upstreams.map((upstream) => {
        if (upstream.id !== upstreamId) return upstream;
        const next = { ...upstream, ...patch };
        if (patch.protocol === "doh3") {
          next.enableHttp3 = true;
          next.dohPath = next.dohPath || "/dns-query";
        } else if (patch.protocol === "doh") {
          next.enableHttp3 = false;
          next.dohPath = next.dohPath || "/dns-query";
        } else if (patch.protocol) {
          next.enableHttp3 = false;
          next.dohPath = undefined;
        }
        if (
          patch.protocol &&
          patch.protocol !== "auto" &&
          patch.protocol !== "tcp" &&
          patch.protocol !== "dot"
        ) {
          next.enablePipeline = false;
        }
        return next;
      }),
    );
  };

  const removeUpstream = (upstreamId: string) => {
    if (selectedGroup.upstreams.length <= 1) return;
    setSelectedUpstreams(
      selectedGroup.upstreams.filter((upstream) => upstream.id !== upstreamId),
    );
  };

  const handleSave = async () => {
    const nextSettings = normalizeStandardDnsSettings(settings);
    const issues = validateStandardDnsSettings(nextSettings, buildInfo);
    if (issues.length > 0) return;
    setSaveError(null);
    try {
      await saveStandardSettings(nextSettings, { apply: true });
      setDraftSettings(nextSettings);
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
    }
  };

  const handleTestUpstream = async (upstream: StandardUpstream) => {
    const resultKey = `${selectedGroup.id}/${upstream.id}`;
    setTestError(null);
    setGroupTestSummary(null);
    setTestingUpstreams((current) => ({ ...current, [resultKey]: true }));
    try {
      const response = await testUpstream({
        upstream: upstreamTestInput(upstream),
        timeoutMs: 5000,
      });
      setTestResults((current) => ({
        ...current,
        [resultKey]: {
          ...response.result,
          id: upstream.id,
          name: upstream.name,
        },
      }));
    } catch (error) {
      setTestResults((current) => ({
        ...current,
        [resultKey]: failedUiTestResult(
          upstream,
          error instanceof Error ? error.message : String(error),
        ),
      }));
    } finally {
      setTestingUpstreams((current) => ({ ...current, [resultKey]: false }));
    }
  };

  const handleTestGroup = async () => {
    if (testableUpstreams.length === 0) return;
    setTestError(null);
    setGroupTestSummary(null);
    setTestingUpstreams((current) => {
      const next = { ...current };
      for (const upstream of testableUpstreams) {
        next[`${selectedGroup.id}/${upstream.id}`] = true;
      }
      return next;
    });
    try {
      const response = await testUpstreamGroup({
        upstreams: testableUpstreams.map(upstreamTestInput),
        timeoutMs: 5000,
      });
      setTestResults((current) => {
        const next = { ...current };
        for (const result of response.results) {
          if (result.id) next[`${selectedGroup.id}/${result.id}`] = result;
        }
        return next;
      });
      setGroupTestSummary(
        response.fastest_upstream_id
          ? t(WEBUI.standardDns.testGroupSummary, {
              success: response.success_count,
              failed: response.failure_count,
              upstream: response.fastest_upstream_id,
              latency: response.fastest_latency_ms ?? 0,
            })
          : t(WEBUI.standardDns.testGroupNoSuccess, {
              failed: response.failure_count,
            }),
      );
    } catch (error) {
      setTestError(error instanceof Error ? error.message : String(error));
    } finally {
      setTestingUpstreams((current) => {
        const next = { ...current };
        for (const upstream of testableUpstreams) {
          next[`${selectedGroup.id}/${upstream.id}`] = false;
        }
        return next;
      });
    }
  };

  return (
    <>
      <AppHeader title={t(WEBUI.standardDns.title)} />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-6xl space-y-6">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <h1 className="text-xl font-semibold tracking-tight">
                {t(WEBUI.standardDns.title)}
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                {t(WEBUI.standardDns.description)}
              </p>
            </div>
            <Button onClick={handleSave} disabled={!canSave}>
              {isBusy ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Save className="size-4" />
              )}
              {isBusy
                ? t(WEBUI.standardDns.savingApplying)
                : t(WEBUI.standardDns.saveApply)}
            </Button>
          </div>

          {validationIssues.length > 0 || saveError ? (
            <ValidationPanel
              issues={validationIssues}
              saveError={saveError}
              protocolLabel={(protocol) => t(protocolLabelKeys[protocol])}
            />
          ) : null}

          {testError ? (
            <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-4 text-sm text-destructive">
              {testError}
            </div>
          ) : null}

          <Card>
            <CardHeader>
              <CardTitle className="text-base">
                {t(WEBUI.standardDns.listenTitle)}
              </CardTitle>
            </CardHeader>
            <CardContent className="grid gap-5 md:grid-cols-[minmax(0,1fr)_auto]">
              <div className="space-y-2">
                <Label htmlFor="standard-listen-address">
                  {t(WEBUI.standardDns.listenAddress)}
                </Label>
                <Input
                  id="standard-listen-address"
                  value={settings.listen.address}
                  onChange={(event) =>
                    setPartial({
                      listen: {
                        ...settings.listen,
                        address: event.target.value,
                      },
                    })
                  }
                  placeholder="0.0.0.0:5335"
                />
              </div>
              <div className="space-y-2">
                <Label>{t(WEBUI.standardDns.listenProtocols)}</Label>
                <div className="flex min-h-8 items-center gap-5 rounded-lg border px-3">
                  <Label className="text-sm font-normal">
                    <Switch
                      checked={settings.listen.udp}
                      onCheckedChange={(checked) =>
                        setPartial({
                          listen: { ...settings.listen, udp: checked },
                        })
                      }
                    />
                    {t(WEBUI.standardDns.udp)}
                  </Label>
                  <Label className="text-sm font-normal">
                    <Switch
                      checked={settings.listen.tcp}
                      onCheckedChange={(checked) =>
                        setPartial({
                          listen: { ...settings.listen, tcp: checked },
                        })
                      }
                    />
                    {t(WEBUI.standardDns.tcp)}
                  </Label>
                </div>
              </div>
            </CardContent>
          </Card>

          <Card id={`group-${selectedGroup.id}`}>
            <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
              <div>
                <CardTitle className="text-base">
                  {t(WEBUI.standardDns.groupsTitle)}
                </CardTitle>
                <p className="mt-1 text-sm text-muted-foreground">
                  {t(WEBUI.standardDns.groupsDescription)}
                </p>
              </div>
              <div className="flex flex-wrap justify-end gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={addGroup}
                >
                  <Plus className="size-4" />
                  {t(WEBUI.standardDns.addGroup)}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={copySelectedGroup}
                >
                  <Copy className="size-4" />
                  {t(WEBUI.standardDns.copyGroup)}
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  disabled={
                    settings.upstreamGroups.length <= 1 ||
                    Boolean(selectedGroup.isDefault) ||
                    groupReferences.length > 0
                  }
                  onClick={removeSelectedGroup}
                >
                  <Trash2 className="size-4" />
                  {t(WEBUI.standardDns.removeGroup)}
                </Button>
              </div>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
                <div className="space-y-2">
                  <Label>{t(WEBUI.standardDns.groupSelect)}</Label>
                  <Select
                    value={selectedGroup.id}
                    onValueChange={setSelectedGroupId}
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {settings.upstreamGroups.map((group) => (
                        <SelectItem key={group.id} value={group.id}>
                          {group.name || group.id}
                          {group.isDefault
                            ? ` · ${t(WEBUI.common.defaultValue)}`
                            : ""}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="space-y-2">
                  <Label htmlFor={`${selectedGroup.id}-group-name`}>
                    {t(WEBUI.standardDns.groupName)}
                  </Label>
                  <Input
                    id={`${selectedGroup.id}-group-name`}
                    value={selectedGroup.name}
                    onChange={(event) =>
                      updateSelectedGroup({ name: event.target.value })
                    }
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor={`${selectedGroup.id}-group-strategy`}>
                    {t(WEBUI.standardDns.groupStrategy)}
                  </Label>
                  <Select
                    value={selectedGroup.strategy}
                    onValueChange={(strategy) =>
                      updateSelectedGroup({
                        strategy: strategy as StandardUpstreamGroup["strategy"],
                      })
                    }
                  >
                    <SelectTrigger
                      id={`${selectedGroup.id}-group-strategy`}
                      className="w-full"
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {(
                        [
                          "balanced",
                          "fastest",
                          "prefer_positive",
                          "consensus",
                        ] as const
                      ).map((strategy) => (
                        <SelectItem key={strategy} value={strategy}>
                          {t(
                            {
                              balanced: WEBUI.standardDns.strategyBalanced,
                              fastest: WEBUI.standardDns.strategyFastest,
                              prefer_positive:
                                WEBUI.standardDns.strategyPreferPositive,
                              consensus: WEBUI.standardDns.strategyConsensus,
                            }[strategy],
                          )}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="flex items-end">
                  <Button
                    type="button"
                    variant={selectedGroup.isDefault ? "secondary" : "outline"}
                    className="w-full"
                    disabled={Boolean(selectedGroup.isDefault)}
                    onClick={setDefaultGroup}
                  >
                    {selectedGroup.isDefault
                      ? t(WEBUI.standardDns.groupDefault)
                      : t(WEBUI.standardDns.setDefaultGroup)}
                  </Button>
                </div>
                <div className="space-y-2 md:col-span-2 xl:col-span-4">
                  <Label htmlFor={`${selectedGroup.id}-group-description`}>
                    {t(WEBUI.standardDns.groupDescription)}
                  </Label>
                  <Input
                    id={`${selectedGroup.id}-group-description`}
                    value={selectedGroup.description ?? ""}
                    onChange={(event) =>
                      updateSelectedGroup({ description: event.target.value })
                    }
                  />
                </div>
              </div>
              <div className="rounded-lg border bg-muted/20 p-3 text-sm">
                <div className="font-medium">
                  {t(WEBUI.standardDns.groupReferences)}
                </div>
                {groupReferences.length > 0 ? (
                  <div className="mt-2 flex flex-wrap gap-2">
                    {groupReferences.map((reference) => (
                      <Badge key={reference.id} variant="secondary" asChild>
                        <Link href={reference.href}>{reference.name}</Link>
                      </Badge>
                    ))}
                  </div>
                ) : (
                  <p className="mt-1 text-muted-foreground">
                    {t(WEBUI.standardDns.groupNoReferences)}
                  </p>
                )}
                <Button
                  asChild
                  variant="link"
                  size="sm"
                  className="mt-1 h-auto px-0"
                >
                  <Link
                    href={
                      groupReferences[0]?.href ??
                      `/standard/routing?group=${encodeURIComponent(selectedGroup.id)}`
                    }
                  >
                    {t(WEBUI.standardDns.openGroupPaths)}
                    <ArrowRight className="size-4" />
                  </Link>
                </Button>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
              <div>
                <CardTitle className="text-base">
                  {t(WEBUI.standardDns.upstreamsTitle)}
                </CardTitle>
                <p className="mt-1 text-sm text-muted-foreground">
                  {t(WEBUI.standardDns.upstreamsDescription)}
                </p>
              </div>
              <div className="flex flex-wrap items-center justify-end gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  disabled={testableUpstreams.length === 0 || isGroupTesting}
                  onClick={handleTestGroup}
                >
                  {isGroupTesting ? (
                    <Loader2 className="size-4 animate-spin" />
                  ) : (
                    <TestTube2 className="size-4" />
                  )}
                  {t(WEBUI.standardDns.testAllUpstreams)}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    setSelectedUpstreams([
                      ...selectedGroup.upstreams,
                      createUpstream(selectedGroup.upstreams),
                    ])
                  }
                >
                  <Plus className="size-4" />
                  {t(WEBUI.standardDns.addUpstream)}
                </Button>
              </div>
            </CardHeader>
            <CardContent className="space-y-3">
              {groupTestSummary ? (
                <div className="rounded-md border bg-muted/20 px-3 py-2 text-sm text-muted-foreground">
                  {groupTestSummary}
                </div>
              ) : null}
              {selectedGroup.upstreams.map((upstream) => (
                <UpstreamEditor
                  key={`${selectedGroup.id}/${upstream.id}`}
                  upstream={upstream}
                  canRemove={selectedGroup.upstreams.length > 1}
                  testResult={testResults[`${selectedGroup.id}/${upstream.id}`]}
                  testing={
                    testingUpstreams[`${selectedGroup.id}/${upstream.id}`] ??
                    false
                  }
                  onChange={(patch) => updateUpstream(upstream.id, patch)}
                  onRemove={() => removeUpstream(upstream.id)}
                  onTest={() => void handleTestUpstream(upstream)}
                />
              ))}
            </CardContent>
          </Card>

          <div className="grid gap-6 lg:grid-cols-2">
            <Card>
              <CardHeader className="flex flex-row items-center justify-between space-y-0">
                <CardTitle className="text-base">
                  {t(WEBUI.standardDns.cacheTitle)}
                </CardTitle>
                {!capabilities.cache ? (
                  <Badge variant="secondary">
                    {t(WEBUI.standardDns.cacheUnsupported)}
                  </Badge>
                ) : null}
              </CardHeader>
              <CardContent className="grid gap-5 sm:grid-cols-2">
                <Label className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal sm:col-span-2">
                  {t(WEBUI.standardDns.cacheEnabled)}
                  <Switch
                    checked={settings.cache.enabled}
                    disabled={!capabilities.cache}
                    onCheckedChange={(checked) =>
                      setPartial({
                        cache: { ...settings.cache, enabled: checked },
                      })
                    }
                  />
                </Label>
                <NumberField
                  id="standard-cache-size"
                  label={t(WEBUI.standardDns.cacheSize)}
                  min={128}
                  value={settings.cache.size}
                  disabled={!settings.cache.enabled || !capabilities.cache}
                  onChange={(value) =>
                    setPartial({
                      cache: {
                        ...settings.cache,
                        size: Math.max(128, Math.trunc(value)),
                      },
                    })
                  }
                />
                <NumberField
                  id="standard-cache-min-ttl"
                  label={t(WEBUI.standardDns.minTtl)}
                  min={0}
                  value={settings.cache.minPositiveTtl}
                  disabled={!settings.cache.enabled || !capabilities.cache}
                  onChange={(value) =>
                    setPartial({
                      cache: {
                        ...settings.cache,
                        minPositiveTtl: Math.max(0, Math.trunc(value)),
                      },
                    })
                  }
                />
                <NumberField
                  id="standard-cache-max-ttl"
                  label={t(WEBUI.standardDns.maxTtl)}
                  min={0}
                  value={settings.cache.maxPositiveTtl}
                  disabled={!settings.cache.enabled || !capabilities.cache}
                  onChange={(value) =>
                    setPartial({
                      cache: {
                        ...settings.cache,
                        maxPositiveTtl: Math.max(0, Math.trunc(value)),
                      },
                    })
                  }
                />
                <NumberField
                  id="standard-cache-negative-ttl"
                  label={t(WEBUI.standardDns.negativeTtl)}
                  min={0}
                  value={settings.cache.maxNegativeTtl}
                  disabled={!settings.cache.enabled || !capabilities.cache}
                  onChange={(value) =>
                    setPartial({
                      cache: {
                        ...settings.cache,
                        maxNegativeTtl: Math.max(0, Math.trunc(value)),
                      },
                    })
                  }
                />
                <NumberField
                  id="standard-cache-negative-ttl-without-soa"
                  label={t(WEBUI.standardDns.negativeTtlWithoutSoa)}
                  min={0}
                  value={settings.cache.negativeTtlWithoutSoa}
                  disabled={!settings.cache.enabled || !capabilities.cache}
                  onChange={(value) =>
                    setPartial({
                      cache: {
                        ...settings.cache,
                        negativeTtlWithoutSoa: Math.max(0, Math.trunc(value)),
                      },
                    })
                  }
                />
              </CardContent>
            </Card>

            <Card>
              <CardHeader className="flex flex-row items-center justify-between space-y-0">
                <CardTitle className="text-base">
                  {t(WEBUI.standardDns.queryLogTitle)}
                </CardTitle>
                {!capabilities.queryRecorder ? (
                  <Badge variant="secondary">
                    {t(WEBUI.standardDns.queryLogUnsupported)}
                  </Badge>
                ) : null}
              </CardHeader>
              <CardContent className="grid gap-5 sm:grid-cols-2">
                <Label className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal sm:col-span-2">
                  {t(WEBUI.standardDns.queryLogEnabled)}
                  <Switch
                    checked={settings.queryLog.enabled}
                    disabled={!capabilities.queryRecorder}
                    onCheckedChange={(checked) =>
                      setPartial({
                        queryLog: {
                          ...settings.queryLog,
                          enabled: checked,
                        },
                      })
                    }
                  />
                </Label>
                <NumberField
                  id="standard-query-log-retention"
                  label={t(WEBUI.standardDns.retentionDays)}
                  min={1}
                  value={settings.queryLog.retentionDays}
                  disabled={
                    !settings.queryLog.enabled || !capabilities.queryRecorder
                  }
                  onChange={(value) =>
                    setPartial({
                      queryLog: {
                        ...settings.queryLog,
                        retentionDays: Math.max(1, Math.trunc(value)),
                      },
                    })
                  }
                />
              </CardContent>
            </Card>
          </div>
        </div>
      </main>
    </>
  );
}

function UpstreamEditor({
  upstream,
  canRemove,
  testResult,
  testing,
  onChange,
  onRemove,
  onTest,
}: {
  upstream: StandardUpstream;
  canRemove: boolean;
  testResult?: UpstreamTestResult;
  testing: boolean;
  onChange: (patch: Partial<StandardUpstream>) => void;
  onRemove: () => void;
  onTest: () => void;
}) {
  const buildInfo = useAppStore((s) => s.buildInfo);
  const { t } = useI18n();
  const usesHttpDns =
    upstream.protocol === "doh" || upstream.protocol === "doh3";
  const usesTls =
    upstream.protocol === "dot" ||
    upstream.protocol === "doh" ||
    upstream.protocol === "doh3" ||
    upstream.protocol === "doq";
  const protocolSupported = isStandardUpstreamProtocolSupported(
    upstream.protocol,
    buildInfo,
  );
  const canTest =
    upstream.enabled &&
    upstream.address.trim() &&
    protocolSupported &&
    !testing;

  return (
    <div className="rounded-lg border bg-card/40 p-4">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <Label className="text-sm font-normal">
          <Switch
            checked={upstream.enabled}
            onCheckedChange={(checked) => onChange({ enabled: checked })}
          />
          {t(WEBUI.standardDns.upstreamEnabled)}
        </Label>
        <div className="flex items-center gap-2">
          {!protocolSupported ? (
            <Badge variant="destructive">
              {t(WEBUI.standardDns.unsupportedProtocol)}
            </Badge>
          ) : null}
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={!canTest}
            onClick={onTest}
          >
            {testing ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <TestTube2 className="size-4" />
            )}
            {testing
              ? t(WEBUI.standardDns.testRunning)
              : t(WEBUI.standardDns.testUpstream)}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={!canRemove}
            onClick={onRemove}
          >
            <Trash2 className="size-4" />
            {t(WEBUI.standardDns.removeUpstream)}
          </Button>
        </div>
      </div>
      <UpstreamTestStatus
        upstream={upstream}
        protocolSupported={protocolSupported}
        result={testResult}
      />
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <div className="space-y-2">
          <Label htmlFor={`${upstream.id}-name`}>
            {t(WEBUI.standardDns.upstreamName)}
          </Label>
          <Input
            id={`${upstream.id}-name`}
            value={upstream.name}
            onChange={(event) => onChange({ name: event.target.value })}
            placeholder={upstream.id}
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor={`${upstream.id}-protocol`}>
            {t(WEBUI.standardDns.upstreamProtocol)}
          </Label>
          <Select
            value={upstream.protocol}
            onValueChange={(value) =>
              onChange({ protocol: value as StandardUpstreamProtocol })
            }
          >
            <SelectTrigger id={`${upstream.id}-protocol`} className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {STANDARD_UPSTREAM_PROTOCOLS.map((protocol) => {
                const supported = isStandardUpstreamProtocolSupported(
                  protocol,
                  buildInfo,
                );
                const required =
                  requiredStandardUpstreamProtocolFeatures(protocol);
                return (
                  <SelectItem
                    key={protocol}
                    value={protocol}
                    disabled={!supported}
                  >
                    <span>{t(protocolLabelKeys[protocol])}</span>
                    {!supported && required.length > 0 ? (
                      <span className="text-xs text-muted-foreground">
                        {t(WEBUI.standardDns.unsupportedProtocolDetail, {
                          features: required.join(", "),
                        })}
                      </span>
                    ) : null}
                  </SelectItem>
                );
              })}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-2 md:col-span-2">
          <Label htmlFor={`${upstream.id}-address`}>
            {t(WEBUI.standardDns.upstreamAddress)}
          </Label>
          <Input
            id={`${upstream.id}-address`}
            value={upstream.address}
            onChange={(event) => onChange({ address: event.target.value })}
            placeholder={usesHttpDns ? "dns.example/dns-query" : "1.1.1.1:53"}
          />
        </div>
        <OptionalTextField
          id={`${upstream.id}-bootstrap`}
          label={t(WEBUI.standardDns.bootstrap)}
          value={upstream.bootstrap ?? ""}
          placeholder="223.5.5.5:53"
          onChange={(value) => onChange({ bootstrap: value })}
        />
        <div className="space-y-2">
          <Label htmlFor={`${upstream.id}-bootstrap-version`}>
            {t(WEBUI.standardDns.bootstrapVersion)}
          </Label>
          <Select
            value={String(upstream.bootstrapVersion ?? 4)}
            onValueChange={(value) =>
              onChange({ bootstrapVersion: Number(value) as 4 | 6 })
            }
          >
            <SelectTrigger
              id={`${upstream.id}-bootstrap-version`}
              className="w-full"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="4">IPv4</SelectItem>
              <SelectItem value="6">IPv6</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <OptionalTextField
          id={`${upstream.id}-dial-address`}
          label={t(WEBUI.standardDns.dialAddress)}
          value={upstream.dialAddress ?? ""}
          placeholder="1.1.1.1"
          onChange={(value) => onChange({ dialAddress: value })}
        />
        <OptionalTextField
          id={`${upstream.id}-outbound`}
          label={t(WEBUI.standardDns.outbound)}
          value={upstream.outbound ?? ""}
          placeholder="private"
          onChange={(value) => onChange({ outbound: value })}
        />
        <OptionalTextField
          id={`${upstream.id}-socks5`}
          label={t(WEBUI.standardDns.socks5)}
          value={upstream.socks5 ?? ""}
          placeholder="127.0.0.1:1080"
          onChange={(value) => onChange({ socks5: value })}
        />
        <OptionalNumberField
          id={`${upstream.id}-timeout`}
          label={t(WEBUI.standardDns.timeoutSeconds)}
          min={1}
          value={upstream.timeoutSeconds}
          onChange={(value) => onChange({ timeoutSeconds: value })}
        />
        <OptionalNumberField
          id={`${upstream.id}-idle-timeout`}
          label={t(WEBUI.standardDns.idleTimeoutSeconds)}
          min={1}
          value={upstream.idleTimeoutSeconds}
          onChange={(value) => onChange({ idleTimeoutSeconds: value })}
        />
        <OptionalNumberField
          id={`${upstream.id}-max-conns`}
          label={t(WEBUI.standardDns.maxConns)}
          min={1}
          max={4096}
          value={upstream.maxConns}
          onChange={(value) => onChange({ maxConns: value })}
        />
        <OptionalNumberField
          id={`${upstream.id}-min-conns`}
          label={t(WEBUI.standardDns.minConns)}
          min={0}
          max={4096}
          value={upstream.minConns}
          onChange={(value) => onChange({ minConns: value })}
        />
        {usesHttpDns ? (
          <OptionalTextField
            id={`${upstream.id}-doh-path`}
            label={t(WEBUI.standardDns.dohPath)}
            value={upstream.dohPath ?? "/dns-query"}
            placeholder="/dns-query"
            onChange={(value) => onChange({ dohPath: value })}
          />
        ) : null}
        {usesTls ? (
          <Label className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal">
            {t(WEBUI.standardDns.tlsVerify)}
            <Switch
              checked={upstream.tlsVerify ?? true}
              onCheckedChange={(checked) => onChange({ tlsVerify: checked })}
            />
          </Label>
        ) : null}
        {upstream.protocol === "auto" ||
        upstream.protocol === "tcp" ||
        upstream.protocol === "dot" ? (
          <Label className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal">
            {t(WEBUI.standardDns.enablePipeline)}
            <Switch
              checked={upstream.enablePipeline ?? false}
              onCheckedChange={(checked) =>
                onChange({ enablePipeline: checked })
              }
            />
          </Label>
        ) : null}
      </div>
    </div>
  );
}

function UpstreamTestStatus({
  upstream,
  protocolSupported,
  result,
}: {
  upstream: StandardUpstream;
  protocolSupported: boolean;
  result?: UpstreamTestResult;
}) {
  const { t } = useI18n();
  if (!upstream.enabled) {
    return (
      <div className="mb-4 text-xs text-muted-foreground">
        {t(WEBUI.standardDns.testDisabledUpstream)}
      </div>
    );
  }
  if (!upstream.address.trim()) {
    return (
      <div className="mb-4 text-xs text-muted-foreground">
        {t(WEBUI.standardDns.testAddressRequired)}
      </div>
    );
  }
  if (!protocolSupported) {
    return (
      <div className="mb-4 text-xs text-muted-foreground">
        {t(WEBUI.standardDns.testProtocolUnsupported)}
      </div>
    );
  }
  if (!result) {
    return (
      <div className="mb-4 text-xs text-muted-foreground">
        {t(WEBUI.standardDns.testNotRun)}
      </div>
    );
  }
  return (
    <div className="mb-4 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
      <Badge variant={result.success ? "secondary" : "destructive"}>
        {result.success
          ? t(WEBUI.standardDns.testSuccess)
          : t(WEBUI.standardDns.testFailed)}
      </Badge>
      {result.protocol ? <span>{result.protocol.toUpperCase()}</span> : null}
      {result.latency_ms !== undefined ? (
        <span>
          {t(WEBUI.standardDns.testLatency, { latency: result.latency_ms })}
        </span>
      ) : null}
      {result.rcode ? <span>RCODE {result.rcode}</span> : null}
      {result.answers.length > 0 ? (
        <span>
          {t(WEBUI.standardDns.testAnswerCount, {
            count: result.answers.length,
          })}
        </span>
      ) : null}
      {result.error_message ? (
        <span className="min-w-0 max-w-full truncate text-destructive">
          {result.error_code === "protocol_unsupported"
            ? t(WEBUI.standardDns.testProtocolUnsupported)
            : result.error_message}
        </span>
      ) : null}
    </div>
  );
}

function OptionalTextField({
  id,
  label,
  value,
  placeholder,
  onChange,
}: {
  id: string;
  label: string;
  value: string;
  placeholder?: string;
  onChange: (value: string) => void;
}) {
  const { t } = useI18n();
  return (
    <div className="space-y-2">
      <Label htmlFor={id}>
        {label}
        <span className="text-xs font-normal text-muted-foreground">
          {t(WEBUI.standardDns.optional)}
        </span>
      </Label>
      <Input
        id={id}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
      />
    </div>
  );
}

function NumberField({
  id,
  label,
  value,
  min,
  max,
  step,
  disabled,
  onChange,
}: {
  id: string;
  label: string;
  value: number;
  min: number;
  max?: number;
  step?: number;
  disabled?: boolean;
  onChange: (value: number) => void;
}) {
  return (
    <div className="space-y-2">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        type="number"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(numberValue(event.target.value, value))}
      />
    </div>
  );
}

function OptionalNumberField({
  id,
  label,
  value,
  min,
  max,
  onChange,
}: {
  id: string;
  label: string;
  value?: number;
  min: number;
  max?: number;
  onChange: (value: number | undefined) => void;
}) {
  const { t } = useI18n();
  return (
    <div className="space-y-2">
      <Label htmlFor={id}>
        {label}
        <span className="text-xs font-normal text-muted-foreground">
          {t(WEBUI.standardDns.optional)}
        </span>
      </Label>
      <Input
        id={id}
        type="number"
        min={min}
        max={max}
        value={value ?? ""}
        onChange={(event) => {
          const raw = event.target.value.trim();
          onChange(raw ? Number(raw) : undefined);
        }}
      />
    </div>
  );
}

function ValidationPanel({
  issues,
  saveError,
  protocolLabel,
}: {
  issues: StandardDnsValidationIssue[];
  saveError: string | null;
  protocolLabel: (protocol: StandardUpstreamProtocol) => string;
}) {
  const { t } = useI18n();
  return (
    <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-4 text-sm text-destructive">
      <div className="font-medium">{t(WEBUI.standardDns.validationTitle)}</div>
      <ul className="mt-2 list-disc space-y-1 pl-5">
        {issues.map((issue, index) => (
          <li key={`${issue.field}-${issue.code}-${index}`}>
            {validationMessage(issue, t, protocolLabel)}
          </li>
        ))}
        {saveError ? <li>{saveError}</li> : null}
      </ul>
    </div>
  );
}

function validationMessage(
  issue: StandardDnsValidationIssue,
  t: (key: string, params?: Record<string, string | number>) => string,
  protocolLabel: (protocol: StandardUpstreamProtocol) => string,
) {
  if (issue.code === "listen_required") {
    return t(WEBUI.standardDns.validationListenRequired);
  }
  if (issue.code === "group_required") {
    return t(WEBUI.standardDns.validationGroupRequired);
  }
  if (issue.code === "group_name_required") {
    return t(WEBUI.standardDns.validationGroupNameRequired);
  }
  if (issue.code === "default_group_invalid") {
    return t(WEBUI.standardDns.validationDefaultGroupInvalid);
  }
  if (issue.code === "upstream_required") {
    return t(WEBUI.standardDns.validationUpstreamRequired);
  }
  if (issue.code === "upstream_address_required") {
    return t(WEBUI.standardDns.validationAddressRequired);
  }
  if (issue.code === "upstream_timeout_invalid") {
    return t(WEBUI.standardDns.validationTimeoutInvalid);
  }
  if (issue.code === "upstream_pool_invalid") {
    return t(WEBUI.standardDns.validationPoolInvalid);
  }
  if (issue.code === "upstream_pipeline_invalid") {
    return t(WEBUI.standardDns.validationPipelineInvalid);
  }
  return t(WEBUI.standardDns.validationProtocolUnsupported, {
    protocol: issue.protocol ? protocolLabel(issue.protocol) : "",
    features: issue.requiredFeatures?.join(", ") ?? "",
  });
}
