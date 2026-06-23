"use client";

import { useMemo, useState } from "react";
import { Loader2, Plus, Save, TestTube2, Trash2 } from "lucide-react";
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
import { createServerSettings } from "@/lib/standard-mode/defaults";
import { upstreamAddress } from "@/lib/standard-mode/generator";
import {
  selectStandardCapabilityMap,
} from "@/lib/standard-mode/selectors";
import type {
  StandardModeSettings,
  StandardServerProtocol,
  StandardServerSettings,
  StandardUpstream,
  StandardUpstreamProtocol,
} from "@/lib/standard-mode/types";
import {
  isStandardServerProtocolSupported,
  isStandardUpstreamProtocolSupported,
  normalizeStandardDnsSettings,
  normalizeStandardUpstream,
  requiredStandardServerProtocolFeatures,
  requiredStandardUpstreamProtocolFeatures,
  STANDARD_SERVER_PROTOCOLS,
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

const serverProtocolLabelKeys: Record<StandardServerProtocol, string> = {
  udp: WEBUI.standardDns.protocolUdp,
  tcp: WEBUI.standardDns.protocolTcp,
  dot: WEBUI.standardDns.protocolDot,
  doh: WEBUI.standardDns.protocolDoh,
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

function createServerId(
  servers: StandardServerSettings[],
  protocol: StandardServerProtocol,
) {
  const used = new Set(servers.map((item) => item.id));
  let index = servers.filter((item) => item.protocol === protocol).length + 1;
  let id = index === 1 && !used.has(protocol) ? protocol : `${protocol}_${index}`;
  while (used.has(id)) {
    index += 1;
    id = `${protocol}_${index}`;
  }
  return id;
}

function createGroupId(groups: StandardModeSettings["upstreamGroups"]) {
  const used = new Set(groups.map((item) => item.id));
  let index = groups.length + 1;
  let id = `group_${index}`;
  while (used.has(id)) {
    index += 1;
    id = `group_${index}`;
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
    ...(normalized.dialAddress ? { dial_addr: normalized.dialAddress } : {}),
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
  const [saveError, setSaveError] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, UpstreamTestResult>>(
    {},
  );
  const [testingUpstreams, setTestingUpstreams] = useState<Record<string, boolean>>(
    {},
  );
  const [groupTestSummary, setGroupTestSummary] = useState<string | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const [newServerProtocol, setNewServerProtocol] =
    useState<StandardServerProtocol>("dot");
  const [hasAttemptedSave, setHasAttemptedSave] = useState(false);
  const settings = draftSettings ?? storeSettings;
  const validationIssues = useMemo(
    () => validateStandardDnsSettings(settings, buildInfo),
    [settings, buildInfo],
  );
  const isBusy = isConfigSaving || isApplying;
  const canSave = !isBusy;
  const showValidationIssues = hasAttemptedSave && validationIssues.length > 0;
  const allTestableUpstreams = settings.upstreamGroups.flatMap((group) =>
    group.upstreams
      .filter(
        (upstream) =>
          upstream.enabled &&
          upstream.address.trim() &&
          isStandardUpstreamProtocolSupported(upstream.protocol, buildInfo),
      )
      .map((upstream) => ({ groupId: group.id, upstream })),
  );
  const isGroupTesting = Object.values(testingUpstreams).some(Boolean);

  const setPartial = (patch: Partial<StandardModeSettings>) => {
    setSaveError(null);
    setDraftSettings((current) => ({ ...(current ?? settings), ...patch }));
  };

  const updateServer = (
    serverId: string,
    patch: Partial<StandardServerSettings>,
  ) => {
    const servers = settings.listen.servers.map((server) =>
      server.id === serverId
        ? patch.protocol
          ? (patch as StandardServerSettings)
          : { ...server, ...patch }
        : server,
    );
    setPartial({
      listen: {
        ...settings.listen,
        address:
          serverId === "udp" && typeof patch.listen === "string"
            ? patch.listen
            : settings.listen.address,
        udp: servers.some((server) => server.protocol === "udp"),
        tcp: servers.some((server) => server.protocol === "tcp"),
        servers,
      },
    });
  };

  const addServer = (protocol: StandardServerProtocol) => {
    const id = createServerId(settings.listen.servers, protocol);
    const servers = [
      ...settings.listen.servers,
      createServerSettings(protocol, id),
    ];
    setPartial({
      listen: {
        ...settings.listen,
        udp: servers.some((server) => server.protocol === "udp"),
        tcp: servers.some((server) => server.protocol === "tcp"),
        servers,
      },
    });
  };

  const removeServer = (serverId: string) => {
    const servers = settings.listen.servers.filter(
      (server) => server.id !== serverId,
    );
    setPartial({
      listen: {
        ...settings.listen,
        udp: servers.some((server) => server.protocol === "udp"),
        tcp: servers.some((server) => server.protocol === "tcp"),
        servers,
      },
    });
  };

  const updateUpstreamGroup = (
    groupId: string,
    patch: Partial<StandardModeSettings["upstreamGroups"][number]>,
  ) => {
    setPartial({
      upstreamGroups: settings.upstreamGroups.map((group) =>
        group.id === groupId
          ? { ...group, ...patch }
          : group,
      ),
    });
  };

  const addUpstreamGroup = () => {
    const id = createGroupId(settings.upstreamGroups);
    setPartial({
      upstreamGroups: [
        ...settings.upstreamGroups,
        {
          id,
          name: id,
          concurrent: 1,
          upstreams: [createUpstream([])],
        },
      ],
    });
  };

  const removeUpstreamGroup = (groupId: string) => {
    const fallbackGroupId = settings.upstreamGroups[0]?.id ?? "default";
    if (groupId === fallbackGroupId || settings.upstreamGroups.length <= 1) return;
    setPartial({
      upstreamGroups: settings.upstreamGroups.filter((group) => group.id !== groupId),
      paths: settings.paths.map((path) =>
        path.upstreamGroupId === groupId
          ? { ...path, upstreamGroupId: fallbackGroupId }
          : path,
      ),
    });
  };

  const setGroupUpstreams = (groupId: string, upstreams: StandardUpstream[]) => {
    updateUpstreamGroup(groupId, { upstreams });
  };

  const updateUpstream = (
    groupId: string,
    upstreamId: string,
    patch: Partial<StandardUpstream>,
  ) => {
    const group = settings.upstreamGroups.find((item) => item.id === groupId);
    if (!group) return;
    setGroupUpstreams(
      groupId,
      group.upstreams.map((upstream) => {
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
        return next;
      }),
    );
  };

  const addUpstreamToGroup = (groupId: string) => {
    const group = settings.upstreamGroups.find((item) => item.id === groupId);
    if (!group) return;
    setGroupUpstreams(groupId, [
      ...group.upstreams,
      createUpstream(group.upstreams),
    ]);
  };

  const removeUpstream = (groupId: string, upstreamId: string) => {
    const group = settings.upstreamGroups.find((item) => item.id === groupId);
    if (!group || group.upstreams.length <= 1) return;
    setGroupUpstreams(
      groupId,
      group.upstreams.filter((upstream) => upstream.id !== upstreamId),
    );
  };

  const handleSave = async () => {
    setHasAttemptedSave(true);
    const issues = validateStandardDnsSettings(settings, buildInfo);
    if (issues.length > 0) return;
    const nextSettings = normalizeStandardDnsSettings(settings);
    setSaveError(null);
    try {
      await saveStandardSettings(nextSettings, { apply: true });
      setDraftSettings(nextSettings);
      setHasAttemptedSave(false);
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
    }
  };

  const handleTestUpstream = async (upstream: StandardUpstream) => {
    setTestError(null);
    setGroupTestSummary(null);
    setTestingUpstreams((current) => ({ ...current, [upstream.id]: true }));
    try {
      const response = await testUpstream({
        upstream: upstreamTestInput(upstream),
        timeoutMs: 5000,
      });
      setTestResults((current) => ({
        ...current,
        [upstream.id]: { ...response.result, id: upstream.id, name: upstream.name },
      }));
    } catch (error) {
      setTestResults((current) => ({
        ...current,
        [upstream.id]: failedUiTestResult(
          upstream,
          error instanceof Error ? error.message : String(error),
        ),
      }));
    } finally {
      setTestingUpstreams((current) => ({ ...current, [upstream.id]: false }));
    }
  };

  const handleTestGroup = async () => {
    if (allTestableUpstreams.length === 0) return;
    setTestError(null);
    setGroupTestSummary(null);
    setTestingUpstreams((current) => {
      const next = { ...current };
      for (const { upstream } of allTestableUpstreams) next[upstream.id] = true;
      return next;
    });
    try {
      const response = await testUpstreamGroup({
        upstreams: allTestableUpstreams.map(({ upstream }) => upstreamTestInput(upstream)),
        timeoutMs: 5000,
      });
      setTestResults((current) => {
        const next = { ...current };
        for (const result of response.results) {
          if (result.id) next[result.id] = result;
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
        for (const { upstream } of allTestableUpstreams) next[upstream.id] = false;
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

          {showValidationIssues || saveError ? (
            <ValidationPanel
              issues={showValidationIssues ? validationIssues : []}
              saveError={saveError}
              protocolLabel={(protocol) => t(protocolLabelKeys[protocol])}
              serverProtocolLabel={(protocol) =>
                t(serverProtocolLabelKeys[protocol])
              }
            />
          ) : null}

          {testError ? (
            <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-4 text-sm text-destructive">
              {testError}
            </div>
          ) : null}

          <Card>
            <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
              <div>
                <CardTitle className="text-base">
                  {t(WEBUI.standardDns.listenTitle)}
                </CardTitle>
              </div>
              <div className="flex flex-wrap items-center justify-end gap-2">
                <Select
                  value={newServerProtocol}
                  onValueChange={(value) =>
                    setNewServerProtocol(value as StandardServerProtocol)
                  }
                >
                  <SelectTrigger className="h-9 w-[132px]">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {STANDARD_SERVER_PROTOCOLS.map((protocol) => (
                      <SelectItem key={protocol} value={protocol}>
                        {t(serverProtocolLabelKeys[protocol])}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => addServer(newServerProtocol)}
                >
                  <Plus className="size-4" />
                  {t(WEBUI.standardDns.addListener)}
                </Button>
              </div>
            </CardHeader>
            <CardContent className="space-y-3">
              {settings.listen.servers.length === 0 ? (
                <div className="rounded-lg border border-dashed bg-muted/20 p-6 text-center text-sm text-muted-foreground">
                  {t(WEBUI.standardDns.noListeners)}
                </div>
              ) : null}
              {settings.listen.servers.map((server) => (
                <ServerProtocolEditor
                  key={server.id}
                  settings={server}
                  onChange={(patch) => updateServer(server.id, patch)}
                  onRemove={() => removeServer(server.id)}
                />
              ))}
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
                  disabled={allTestableUpstreams.length === 0 || isGroupTesting}
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
                  onClick={addUpstreamGroup}
                >
                  <Plus className="size-4" />
                  {t(WEBUI.standardDns.addUpstreamGroup)}
                </Button>
              </div>
            </CardHeader>
            <CardContent className="space-y-3">
              {groupTestSummary ? (
                <div className="rounded-md border bg-muted/20 px-3 py-2 text-sm text-muted-foreground">
                  {groupTestSummary}
                </div>
              ) : null}
              {settings.upstreamGroups.map((group, index) => (
                <UpstreamGroupEditor
                  key={group.id}
                  group={group}
                  isDefault={index === 0 || group.isDefault === true}
                  canRemove={index > 0}
                  testResults={testResults}
                  testingUpstreams={testingUpstreams}
                  onChange={(patch) => updateUpstreamGroup(group.id, patch)}
                  onAddUpstream={() => addUpstreamToGroup(group.id)}
                  onRemove={() => removeUpstreamGroup(group.id)}
                  onUpdateUpstream={(upstreamId, patch) =>
                    updateUpstream(group.id, upstreamId, patch)
                  }
                  onRemoveUpstream={(upstreamId) =>
                    removeUpstream(group.id, upstreamId)
                  }
                  onTestUpstream={(upstream) => void handleTestUpstream(upstream)}
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
                  value={settings.cache.minTtl}
                  disabled={!settings.cache.enabled || !capabilities.cache}
                  onChange={(value) =>
                    setPartial({
                      cache: {
                        ...settings.cache,
                        minTtl: Math.max(0, Math.trunc(value)),
                      },
                    })
                  }
                />
                <NumberField
                  id="standard-cache-max-ttl"
                  label={t(WEBUI.standardDns.maxTtl)}
                  min={0}
                  value={settings.cache.maxTtl}
                  disabled={!settings.cache.enabled || !capabilities.cache}
                  onChange={(value) =>
                    setPartial({
                      cache: {
                        ...settings.cache,
                        maxTtl: Math.max(0, Math.trunc(value)),
                      },
                    })
                  }
                />
                <NumberField
                  id="standard-cache-negative-ttl"
                  label={t(WEBUI.standardDns.negativeTtl)}
                  min={0}
                  value={settings.cache.negativeTtl}
                  disabled={!settings.cache.enabled || !capabilities.cache}
                  onChange={(value) =>
                    setPartial({
                      cache: {
                        ...settings.cache,
                        negativeTtl: Math.max(0, Math.trunc(value)),
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
                <NumberField
                  id="standard-query-log-sample-rate"
                  label={t(WEBUI.standardDns.sampleRate)}
                  min={0}
                  max={1}
                  step={0.01}
                  value={settings.queryLog.sampleRate}
                  disabled={
                    !settings.queryLog.enabled || !capabilities.queryRecorder
                  }
                  onChange={(value) =>
                    setPartial({
                      queryLog: {
                        ...settings.queryLog,
                        sampleRate: Math.min(1, Math.max(0, value)),
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

function ServerProtocolEditor({
  settings,
  onChange,
  onRemove,
}: {
  settings: StandardServerSettings;
  onChange: (patch: Partial<StandardServerSettings>) => void;
  onRemove: () => void;
}) {
  const buildInfo = useAppStore((s) => s.buildInfo);
  const { t } = useI18n();
  const protocol = settings.protocol;
  const supported = isStandardServerProtocolSupported(
    protocol,
    buildInfo,
    settings,
  );
  const required = requiredStandardServerProtocolFeatures(protocol, settings);
  const usesTls = protocol === "dot" || protocol === "doh" || protocol === "doq";
  const usesHttp = protocol === "doh";
  const usesIdleTimeout = protocol !== "udp";

  return (
    <div className="rounded-lg border bg-card/40 p-4">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <Select
            value={protocol}
            onValueChange={(value) => {
              const next = createServerSettings(
                value as StandardServerProtocol,
                settings.id,
              );
              onChange(next);
            }}
          >
            <SelectTrigger className="h-9 w-[132px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {STANDARD_SERVER_PROTOCOLS.map((item) => (
                <SelectItem key={item} value={item}>
                  {t(serverProtocolLabelKeys[item])}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <span className="font-mono text-xs text-muted-foreground">
            {settings.id}
          </span>
          {!supported ? (
            <Badge variant="destructive">
              {required.length > 0
                ? t(WEBUI.standardDns.unsupportedProtocolDetail, {
                    features: required.join(", "),
                  })
                : t(WEBUI.standardDns.unsupportedProtocol)}
            </Badge>
          ) : null}
        </div>
        <Button type="button" variant="ghost" size="sm" onClick={onRemove}>
          <Trash2 className="size-4" />
          {t(WEBUI.standardDns.removeListener)}
        </Button>
      </div>
      <div className="grid gap-4 md:grid-cols-2">
        <div className="space-y-2 md:col-span-2">
          <Label htmlFor={`standard-${protocol}-listen`}>
            {t(WEBUI.standardDns.listenAddress)}
            <RequiredMark />
          </Label>
          <Input
            id={`standard-${protocol}-listen`}
            value={settings.listen}
            onChange={(event) => onChange({ listen: event.target.value })}
            placeholder={serverListenPlaceholder(protocol)}
          />
        </div>
        {usesHttp ? (
          <div className="space-y-2">
            <Label htmlFor={`standard-${protocol}-path`}>
              {t(WEBUI.standardDns.dohPath)}
              <RequiredMark />
            </Label>
            <Input
              id={`standard-${protocol}-path`}
              value={settings.path ?? "/dns-query"}
              onChange={(event) => onChange({ path: event.target.value })}
              placeholder="/dns-query"
            />
          </div>
        ) : null}
        {usesIdleTimeout ? (
          <NumberField
            id={`standard-${protocol}-idle-timeout`}
            label={t(WEBUI.standardDns.idleTimeout)}
            min={1}
            value={settings.idleTimeout ?? (protocol === "doh" ? 30 : 10)}
            onChange={(value) =>
              onChange({ idleTimeout: Math.max(1, Math.trunc(value)) })
            }
          />
        ) : null}
        {usesTls ? (
          <>
            <div className="space-y-2">
              <Label htmlFor={`standard-${protocol}-cert`}>
                {t(WEBUI.standardDns.tlsCert)}
                <RequiredMark />
              </Label>
              <Input
                id={`standard-${protocol}-cert`}
                value={settings.cert ?? ""}
                onChange={(event) => onChange({ cert: event.target.value })}
                placeholder="/etc/oxidns/server.crt"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor={`standard-${protocol}-key`}>
                {t(WEBUI.standardDns.tlsKey)}
                <RequiredMark />
              </Label>
              <Input
                id={`standard-${protocol}-key`}
                value={settings.key ?? ""}
                onChange={(event) => onChange({ key: event.target.value })}
                placeholder="/etc/oxidns/server.key"
              />
            </div>
          </>
        ) : null}
        {usesHttp ? (
          <>
            <OptionalTextField
              id={`standard-${protocol}-src-ip-header`}
              label={t(WEBUI.standardDns.srcIpHeader)}
              value={settings.srcIpHeader ?? ""}
              placeholder="X-Forwarded-For"
              onChange={(value) => onChange({ srcIpHeader: value })}
            />
            <Label className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal">
              {t(WEBUI.standardDns.enableHttp3)}
              <Switch
                checked={settings.enableHttp3 ?? false}
                onCheckedChange={(checked) => onChange({ enableHttp3: checked })}
              />
            </Label>
          </>
        ) : null}
      </div>
    </div>
  );
}

function serverListenPlaceholder(protocol: StandardServerProtocol): string {
  if (protocol === "udp" || protocol === "tcp") return "0.0.0.0:5335";
  if (protocol === "doh") return "0.0.0.0:443";
  return "0.0.0.0:853";
}

function RequiredMark() {
  return (
    <span aria-hidden="true" className="ml-1 text-destructive">
      *
    </span>
  );
}

function UpstreamGroupEditor({
  group,
  isDefault,
  canRemove,
  testResults,
  testingUpstreams,
  onChange,
  onAddUpstream,
  onRemove,
  onUpdateUpstream,
  onRemoveUpstream,
  onTestUpstream,
}: {
  group: StandardModeSettings["upstreamGroups"][number];
  isDefault: boolean;
  canRemove: boolean;
  testResults: Record<string, UpstreamTestResult>;
  testingUpstreams: Record<string, boolean>;
  onChange: (patch: Partial<StandardModeSettings["upstreamGroups"][number]>) => void;
  onAddUpstream: () => void;
  onRemove: () => void;
  onUpdateUpstream: (
    upstreamId: string,
    patch: Partial<StandardUpstream>,
  ) => void;
  onRemoveUpstream: (upstreamId: string) => void;
  onTestUpstream: (upstream: StandardUpstream) => void;
}) {
  const { t } = useI18n();
  return (
    <div className="rounded-lg border bg-card/40 p-4">
      <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
        <div className="grid min-w-0 flex-1 gap-4 md:grid-cols-2">
          <div className="space-y-2">
            <Label htmlFor={`${group.id}-group-name`}>
              {t(WEBUI.standardDns.upstreamGroupName)}
            </Label>
            <Input
              id={`${group.id}-group-name`}
              value={group.name}
              onChange={(event) => onChange({ name: event.target.value })}
              placeholder={group.id}
            />
          </div>
          <div className="space-y-2">
            <NumberField
              id={`${group.id}-group-concurrent`}
              label={t(WEBUI.standardDns.upstreamGroupConcurrent)}
              min={1}
              max={3}
              value={group.concurrent}
              onChange={(value) =>
                onChange({ concurrent: Math.max(1, Math.min(3, Math.trunc(value))) })
              }
            />
          </div>
        </div>
        <div className="flex flex-wrap items-center justify-end gap-2">
          {isDefault ? (
            <Badge variant="secondary">{t(WEBUI.common.defaultValue)}</Badge>
          ) : null}
          <Button type="button" variant="outline" size="sm" onClick={onAddUpstream}>
            <Plus className="size-4" />
            {t(WEBUI.standardDns.addUpstream)}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={!canRemove}
            onClick={onRemove}
          >
            <Trash2 className="size-4" />
            {t(WEBUI.standardDns.removeUpstreamGroup)}
          </Button>
        </div>
      </div>
      <div className="space-y-3">
        {group.upstreams.map((upstream) => (
          <UpstreamEditor
            key={upstream.id}
            upstream={upstream}
            canRemove={group.upstreams.length > 1}
            testResult={testResults[upstream.id]}
            testing={testingUpstreams[upstream.id] ?? false}
            onChange={(patch) => onUpdateUpstream(upstream.id, patch)}
            onRemove={() => onRemoveUpstream(upstream.id)}
            onTest={() => onTestUpstream(upstream)}
          />
        ))}
      </div>
    </div>
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
  const usesHttpDns = upstream.protocol === "doh" || upstream.protocol === "doh3";
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
    upstream.enabled && upstream.address.trim() && protocolSupported && !testing;

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
            {upstream.enabled ? <RequiredMark /> : null}
          </Label>
          <Input
            id={`${upstream.id}-address`}
            value={upstream.address}
            onChange={(event) => onChange({ address: event.target.value })}
            placeholder={
              usesHttpDns ? "dns.example/dns-query" : "1.1.1.1:53"
            }
          />
        </div>
        <OptionalTextField
          id={`${upstream.id}-bootstrap`}
          label={t(WEBUI.standardDns.bootstrap)}
          value={upstream.bootstrap ?? ""}
          placeholder="223.5.5.5:53"
          onChange={(value) => onChange({ bootstrap: value })}
        />
        <OptionalTextField
          id={`${upstream.id}-dial-address`}
          label={t(WEBUI.standardDns.dialAddress)}
          value={upstream.dialAddress ?? ""}
          placeholder="1.1.1.1:853"
          onChange={(value) => onChange({ dialAddress: value })}
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

function ValidationPanel({
  issues,
  saveError,
  protocolLabel,
  serverProtocolLabel,
}: {
  issues: StandardDnsValidationIssue[];
  saveError: string | null;
  protocolLabel: (protocol: StandardUpstreamProtocol) => string;
  serverProtocolLabel: (protocol: StandardServerProtocol) => string;
}) {
  const { t } = useI18n();
  return (
    <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-4 text-sm text-destructive">
      <div className="font-medium">{t(WEBUI.standardDns.validationTitle)}</div>
      <ul className="mt-2 list-disc space-y-1 pl-5">
        {issues.map((issue, index) => (
          <li key={`${issue.field}-${issue.code}-${index}`}>
            {validationMessage(issue, t, protocolLabel, serverProtocolLabel)}
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
  serverProtocolLabel: (protocol: StandardServerProtocol) => string,
) {
  if (issue.code === "listen_required") {
    return t(WEBUI.standardDns.validationListenRequired);
  }
  if (issue.code === "server_listen_required") {
    return t(WEBUI.standardDns.validationServerListenRequired, {
      protocol: issue.serverProtocol
        ? serverProtocolLabel(issue.serverProtocol)
        : "",
    });
  }
  if (issue.code === "server_tls_required") {
    return t(WEBUI.standardDns.validationServerTlsRequired, {
      protocol: issue.serverProtocol
        ? serverProtocolLabel(issue.serverProtocol)
        : "",
    });
  }
  if (issue.code === "doh_path_required") {
    return t(WEBUI.standardDns.validationDohPathRequired);
  }
  if (issue.code === "server_protocol_unsupported") {
    return t(WEBUI.standardDns.validationServerProtocolUnsupported, {
      protocol: issue.serverProtocol
        ? serverProtocolLabel(issue.serverProtocol)
        : "",
      features: issue.requiredFeatures?.join(", ") || "-",
    });
  }
  if (issue.code === "server_port_conflict") {
    return t(WEBUI.standardDns.validationServerPortConflict, {
      protocol: issue.serverProtocol
        ? serverProtocolLabel(issue.serverProtocol)
        : "",
      id: issue.conflictWith ?? "",
    });
  }
  if (issue.code === "upstream_required") {
    return t(WEBUI.standardDns.validationUpstreamRequired);
  }
  if (issue.code === "upstream_address_required") {
    return t(WEBUI.standardDns.validationAddressRequired);
  }
  return t(WEBUI.standardDns.validationProtocolUnsupported, {
    protocol: issue.protocol ? protocolLabel(issue.protocol) : "",
    features: issue.requiredFeatures?.join(", ") ?? "",
  });
}
