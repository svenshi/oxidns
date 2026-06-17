"use client";

import { useMemo, useState } from "react";
import { Loader2, Plus, Save, Trash2 } from "lucide-react";
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
  selectDefaultUpstreamGroup,
  selectStandardCapabilityMap,
} from "@/lib/standard-mode/selectors";
import type {
  StandardModeSettings,
  StandardUpstream,
  StandardUpstreamProtocol,
} from "@/lib/standard-mode/types";
import {
  isStandardUpstreamProtocolSupported,
  normalizeStandardDnsSettings,
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

function numberValue(value: string, fallback: number) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
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
  const settings = draftSettings ?? storeSettings;
  const defaultGroup = selectDefaultUpstreamGroup(settings);
  const validationIssues = useMemo(
    () => validateStandardDnsSettings(settings, buildInfo),
    [settings, buildInfo],
  );
  const isBusy = isConfigSaving || isApplying;
  const canSave = validationIssues.length === 0 && !isBusy;

  const setPartial = (patch: Partial<StandardModeSettings>) => {
    setSaveError(null);
    setDraftSettings((current) => ({ ...(current ?? settings), ...patch }));
  };

  const setDefaultUpstreams = (upstreams: StandardUpstream[]) => {
    const defaultGroupId = defaultGroup.id;
    setPartial({
      upstreamGroups: settings.upstreamGroups.map((group, index) =>
        group.id === defaultGroupId || (index === 0 && defaultGroupId === group.id)
          ? { ...group, upstreams }
          : group,
      ),
    });
  };

  const updateUpstream = (
    upstreamId: string,
    patch: Partial<StandardUpstream>,
  ) => {
    setDefaultUpstreams(
      defaultGroup.upstreams.map((upstream) => {
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

  const removeUpstream = (upstreamId: string) => {
    if (defaultGroup.upstreams.length <= 1) return;
    setDefaultUpstreams(
      defaultGroup.upstreams.filter((upstream) => upstream.id !== upstreamId),
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
              <Button
                variant="outline"
                size="sm"
                onClick={() =>
                  setDefaultUpstreams([
                    ...defaultGroup.upstreams,
                    createUpstream(defaultGroup.upstreams),
                  ])
                }
              >
                <Plus className="size-4" />
                {t(WEBUI.standardDns.addUpstream)}
              </Button>
            </CardHeader>
            <CardContent className="space-y-3">
              {defaultGroup.upstreams.map((upstream) => (
                <UpstreamEditor
                  key={upstream.id}
                  upstream={upstream}
                  canRemove={defaultGroup.upstreams.length > 1}
                  onChange={(patch) => updateUpstream(upstream.id, patch)}
                  onRemove={() => removeUpstream(upstream.id)}
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

function UpstreamEditor({
  upstream,
  canRemove,
  onChange,
  onRemove,
}: {
  upstream: StandardUpstream;
  canRemove: boolean;
  onChange: (patch: Partial<StandardUpstream>) => void;
  onRemove: () => void;
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
