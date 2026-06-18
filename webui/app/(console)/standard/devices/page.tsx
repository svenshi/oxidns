"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import {
  Baby,
  Loader2,
  Plus,
  RefreshCw,
  Save,
  ShieldOff,
  Smartphone,
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import {
  fetchQueryRecorderTopClients,
  fetchQueryRecords,
  type QueryRecordRow,
  type QueryRecorderTopRow,
} from "@/lib/oxidns-api";
import type {
  StandardDeviceProfile,
  StandardModeSettings,
} from "@/lib/standard-mode/types";
import {
  normalizeStandardDeviceSettings,
  standardDeviceCapabilityMap,
  validateStandardDeviceSettings,
  type StandardDeviceValidationIssue,
} from "@/lib/standard-mode/validation";
import { useAppStore } from "@/lib/store";

const UNASSIGNED_PATH = "__inherit__";

const policyLabelKeys: Record<NonNullable<StandardDeviceProfile["filtering"]>, string> = {
  inherit: WEBUI.standardDevices.policyInherit,
  enabled: WEBUI.standardDevices.policyEnabled,
  disabled: WEBUI.standardDevices.policyDisabled,
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

function nextDeviceId(devices: StandardDeviceProfile[], address?: string) {
  const base = `device_${(address || "")
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "_")
    .replace(/^_+|_+$/g, "") || devices.length + 1}`;
  const used = new Set(devices.map((device) => device.id));
  if (!used.has(base)) return base;
  let index = 2;
  let id = `${base}_${index}`;
  while (used.has(id)) {
    index += 1;
    id = `${base}_${index}`;
  }
  return id;
}

function createDevice(
  devices: StandardDeviceProfile[],
  name: string,
  addresses: string[] = [],
): StandardDeviceProfile {
  const id = nextDeviceId(devices, addresses[0]);
  return {
    id,
    name: name || id,
    addresses,
    filtering: "inherit",
    queryLog: "inherit",
  };
}

function firstRecordableAddress(device: StandardDeviceProfile | undefined) {
  return device?.addresses.find((address) => address && !address.includes("/"));
}

function questionName(record: QueryRecordRow) {
  return record.questions_json[0]?.name ?? "-";
}

function questionType(record: QueryRecordRow) {
  return record.questions_json[0]?.qtype ?? "-";
}

export default function StandardDevicesPage() {
  const storeSettings = useAppStore((s) => s.standardSettings);
  const buildInfo = useAppStore((s) => s.buildInfo);
  const plugins = useAppStore((s) => s.plugins);
  const saveStandardSettings = useAppStore((s) => s.saveStandardSettings);
  const isConfigSaving = useAppStore((s) => s.isConfigSaving);
  const isApplying = useAppStore((s) => s.isApplying);
  const { t, formatDateTime, formatNumber } = useI18n();
  const recorder =
    plugins.find((plugin) => plugin.name === "standard_recorder") ??
    plugins.find((plugin) => plugin.pluginKind === "query_recorder");
  const recorderName = recorder?.name;
  const [draftSettings, setDraftSettings] =
    useState<StandardModeSettings | null>(null);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(
    storeSettings.devices[0]?.id ?? null,
  );
  const [topClients, setTopClients] = useState<QueryRecorderTopRow[]>([]);
  const [deviceRecords, setDeviceRecords] = useState<QueryRecordRow[]>([]);
  const [loadingClients, setLoadingClients] = useState(false);
  const [loadingRecords, setLoadingRecords] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const requestIdRef = useRef(0);
  const settings = draftSettings ?? storeSettings;
  const capabilities = useMemo(
    () => standardDeviceCapabilityMap(buildInfo),
    [buildInfo],
  );
  const validationIssues = useMemo(
    () => validateStandardDeviceSettings(settings, buildInfo),
    [settings, buildInfo],
  );
  const selectedDevice =
    settings.devices.find((device) => device.id === selectedDeviceId) ??
    settings.devices[0];
  const isBusy = isConfigSaving || isApplying;
  const canSave = validationIssues.length === 0 && !isBusy;
  const assignedDevices = settings.devices.filter((device) => device.assignedPathId);
  const policyDevices = settings.devices.filter(
    (device) =>
      device.assignedPathId ||
      device.filtering === "enabled" ||
      device.filtering === "disabled" ||
      device.queryLog === "enabled" ||
      device.queryLog === "disabled",
  );

  const setSettings = (nextSettings: StandardModeSettings) => {
    setSaveError(null);
    setActionMessage(null);
    setDraftSettings(nextSettings);
  };

  const setDevices = (devices: StandardDeviceProfile[]) => {
    setSettings({ ...settings, devices });
  };

  const updateDevice = (
    deviceId: string,
    patch: Partial<StandardDeviceProfile>,
  ) => {
    setDevices(
      settings.devices.map((device) =>
        device.id === deviceId ? { ...device, ...patch } : device,
      ),
    );
  };

  const addManualDevice = () => {
    const next = createDevice(settings.devices, t(WEBUI.standardDevices.newDeviceName));
    setDevices([...settings.devices, next]);
    setSelectedDeviceId(next.id);
  };

  const addClientDevice = (clientIp: string) => {
    const existing = settings.devices.find((device) =>
      device.addresses.includes(clientIp),
    );
    if (existing) {
      setSelectedDeviceId(existing.id);
      return;
    }
    const next = createDevice(settings.devices, clientIp, [clientIp]);
    setDevices([...settings.devices, next]);
    setSelectedDeviceId(next.id);
  };

  const removeDevice = (deviceId: string) => {
    const next = settings.devices.filter((device) => device.id !== deviceId);
    setDevices(next);
    if (selectedDeviceId === deviceId) {
      setSelectedDeviceId(next[0]?.id ?? null);
    }
  };

  const applyChildProtection = (deviceId: string) => {
    updateDevice(deviceId, { filtering: "enabled" });
  };

  const applyPrivacy = (deviceId: string) => {
    const targetPath = settings.paths[1]?.id ?? settings.paths[0]?.id;
    updateDevice(deviceId, {
      assignedPathId: targetPath,
      queryLog: "disabled",
    });
  };

  const loadClients = async () => {
    if (!recorderName) return;
    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;
    setLoadingClients(true);
    setLoadError(null);
    try {
      const response = await fetchQueryRecorderTopClients(recorderName, {
        limit: 12,
      });
      if (requestIdRef.current !== requestId) return;
      setTopClients(response.rows);
    } catch (error) {
      if (requestIdRef.current !== requestId) return;
      setLoadError(
        error instanceof Error ? error.message : t(WEBUI.standardDevices.readFailed),
      );
    } finally {
      if (requestIdRef.current === requestId) setLoadingClients(false);
    }
  };

  const loadDeviceRecords = async (device: StandardDeviceProfile | undefined) => {
    const clientIp = firstRecordableAddress(device);
    if (!recorderName || !clientIp) {
      setDeviceRecords([]);
      return;
    }
    setLoadingRecords(true);
    setLoadError(null);
    try {
      const response = await fetchQueryRecords(recorderName, {
        limit: 10,
        clientIp,
      });
      setDeviceRecords(response.records);
    } catch (error) {
      setLoadError(
        error instanceof Error ? error.message : t(WEBUI.standardDevices.readFailed),
      );
    } finally {
      setLoadingRecords(false);
    }
  };

  useEffect(() => {
    if (!recorderName) return;
    const timer = window.setTimeout(() => void loadClients(), 0);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recorderName]);

  useEffect(() => {
    const timer = window.setTimeout(
      () => void loadDeviceRecords(selectedDevice),
      0,
    );
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recorderName, selectedDeviceId]);

  const handleSave = async () => {
    const nextSettings = normalizeStandardDeviceSettings(settings);
    const issues = validateStandardDeviceSettings(nextSettings, buildInfo);
    if (issues.length > 0) return;
    setSaveError(null);
    setActionMessage(null);
    try {
      await saveStandardSettings(nextSettings, { apply: true });
      setDraftSettings(nextSettings);
      setActionMessage(t(WEBUI.standardDevices.saved));
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <>
      <AppHeader title={t(WEBUI.standardDevices.title)} />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-7xl space-y-6">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <h1 className="text-xl font-semibold tracking-tight">
                {t(WEBUI.standardDevices.title)}
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                {t(WEBUI.standardDevices.description)}
              </p>
            </div>
            <div className="flex flex-wrap gap-2">
              <Button variant="outline" onClick={addManualDevice}>
                <Plus className="size-4" />
                {t(WEBUI.standardDevices.addDevice)}
              </Button>
              <Button onClick={handleSave} disabled={!canSave}>
                {isBusy ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : (
                  <Save className="size-4" />
                )}
                {isBusy
                  ? t(WEBUI.standardDevices.savingApplying)
                  : t(WEBUI.standardDevices.saveApply)}
              </Button>
            </div>
          </div>

          {validationIssues.length > 0 || saveError ? (
            <ValidationPanel issues={validationIssues} saveError={saveError} />
          ) : null}
          {actionMessage ? (
            <div className="rounded-lg border border-primary/30 bg-primary/10 p-3 text-sm text-primary">
              {actionMessage}
            </div>
          ) : null}
          {!capabilities.sequence || !capabilities.clientIp ? (
            <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-700 dark:text-amber-300">
              {t(WEBUI.standardDevices.validationCapabilityRequired)}
            </div>
          ) : null}
          {loadError ? (
            <div className="rounded-lg border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive">
              {loadError}
            </div>
          ) : null}

          <div className="grid gap-3 sm:grid-cols-3">
            <MetricCard
              label={t(WEBUI.standardDevices.deviceCount)}
              value={formatNumber(settings.devices.length)}
            />
            <MetricCard
              label={t(WEBUI.standardDevices.assignedCount)}
              value={formatNumber(assignedDevices.length)}
            />
            <MetricCard
              label={t(WEBUI.standardDevices.policyCount)}
              value={formatNumber(policyDevices.length)}
            />
          </div>

          <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_380px]">
            <div className="space-y-4">
              {settings.devices.length === 0 ? (
                <Card>
                  <CardContent className="p-6 text-sm text-muted-foreground">
                    {t(WEBUI.standardDevices.emptyDevices)}
                  </CardContent>
                </Card>
              ) : (
                settings.devices.map((device) => (
                  <DeviceEditor
                    key={device.id}
                    device={device}
                    settings={settings}
                    selected={selectedDevice?.id === device.id}
                    onSelect={() => setSelectedDeviceId(device.id)}
                    onUpdate={(patch) => updateDevice(device.id, patch)}
                    onRemove={() => removeDevice(device.id)}
                    onChildProtection={() => applyChildProtection(device.id)}
                    onPrivacy={() => applyPrivacy(device.id)}
                  />
                ))
              )}
            </div>

            <div className="space-y-4">
              <Card>
                <CardHeader className="flex flex-row items-center justify-between space-y-0">
                  <CardTitle className="flex items-center gap-2 text-base">
                    <Smartphone className="size-4" />
                    {t(WEBUI.standardDevices.recentClientsTitle)}
                  </CardTitle>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void loadClients()}
                    disabled={!recorderName || loadingClients}
                  >
                    {loadingClients ? (
                      <Loader2 className="size-4 animate-spin" />
                    ) : (
                      <RefreshCw className="size-4" />
                    )}
                    {t(WEBUI.common.refresh)}
                  </Button>
                </CardHeader>
                <CardContent>
                  {!recorderName ? (
                    <p className="text-sm text-muted-foreground">
                      {t(WEBUI.standardDevices.recorderUnavailable)}
                    </p>
                  ) : topClients.length === 0 ? (
                    <p className="text-sm text-muted-foreground">
                      {loadingClients
                        ? t(WEBUI.common.loading)
                        : t(WEBUI.standardDevices.noRecentClients)}
                    </p>
                  ) : (
                    <div className="space-y-2">
                      {topClients.map((client) => (
                        <div
                          key={client.key}
                          className="flex min-w-0 items-center justify-between gap-3 rounded-lg border p-3"
                        >
                          <div className="min-w-0">
                            <div className="truncate font-mono text-sm">
                              {client.key}
                            </div>
                            <div className="text-xs text-muted-foreground">
                              {t(WEBUI.standardDevices.clientQueryCount, {
                                count: formatNumber(client.count),
                              })}
                            </div>
                          </div>
                          <Button
                            variant="outline"
                            size="sm"
                            onClick={() => addClientDevice(client.key)}
                          >
                            <Plus className="size-4" />
                            {t(WEBUI.standardDevices.nameClient)}
                          </Button>
                        </div>
                      ))}
                    </div>
                  )}
                </CardContent>
              </Card>

              <Card>
                <CardHeader className="flex flex-row items-center justify-between space-y-0">
                  <CardTitle className="text-base">
                    {t(WEBUI.standardDevices.deviceQueriesTitle)}
                  </CardTitle>
                  <Badge variant="outline">
                    {selectedDevice?.name ?? t(WEBUI.common.empty)}
                  </Badge>
                </CardHeader>
                <CardContent>
                  {!selectedDevice ? (
                    <p className="text-sm text-muted-foreground">
                      {t(WEBUI.standardDevices.selectDeviceHint)}
                    </p>
                  ) : !firstRecordableAddress(selectedDevice) ? (
                    <p className="text-sm text-muted-foreground">
                      {t(WEBUI.standardDevices.recordAddressRequired)}
                    </p>
                  ) : loadingRecords ? (
                    <p className="text-sm text-muted-foreground">
                      {t(WEBUI.common.loading)}
                    </p>
                  ) : deviceRecords.length === 0 ? (
                    <p className="text-sm text-muted-foreground">
                      {t(WEBUI.standardDevices.noDeviceQueries)}
                    </p>
                  ) : (
                    <div className="overflow-x-auto">
                      <Table>
                        <TableHeader>
                          <TableRow>
                            <TableHead>{t(WEBUI.standardDevices.timeColumn)}</TableHead>
                            <TableHead>{t(WEBUI.standardDevices.domainColumn)}</TableHead>
                            <TableHead>{t(WEBUI.standardDevices.qtypeColumn)}</TableHead>
                            <TableHead>{t(WEBUI.standardDevices.resultColumn)}</TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          {deviceRecords.map((record) => (
                            <TableRow key={record.id}>
                              <TableCell className="whitespace-nowrap text-xs text-muted-foreground">
                                {formatDateTime(record.created_at_ms)}
                              </TableCell>
                              <TableCell className="max-w-[180px] truncate font-mono text-xs">
                                {questionName(record)}
                              </TableCell>
                              <TableCell className="text-xs">
                                {questionType(record)}
                              </TableCell>
                              <TableCell className="text-xs">
                                {record.error ?? record.rcode ?? "-"}
                              </TableCell>
                            </TableRow>
                          ))}
                        </TableBody>
                      </Table>
                    </div>
                  )}
                </CardContent>
              </Card>
            </div>
          </div>
        </div>
      </main>
    </>
  );
}

function DeviceEditor({
  device,
  settings,
  selected,
  onSelect,
  onUpdate,
  onRemove,
  onChildProtection,
  onPrivacy,
}: {
  device: StandardDeviceProfile;
  settings: StandardModeSettings;
  selected: boolean;
  onSelect: () => void;
  onUpdate: (patch: Partial<StandardDeviceProfile>) => void;
  onRemove: () => void;
  onChildProtection: () => void;
  onPrivacy: () => void;
}) {
  const { t } = useI18n();
  return (
    <Card className={selected ? "border-primary/50" : undefined}>
      <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
        <div className="min-w-0">
          <CardTitle className="flex items-center gap-2 text-base">
            <Smartphone className="size-4" />
            <span className="truncate">{device.name || device.id}</span>
          </CardTitle>
          <div className="mt-2 flex flex-wrap gap-2">
            <Badge variant="secondary">
              {t(WEBUI.standardDevices.addressCount, {
                count: device.addresses.length,
              })}
            </Badge>
            {device.assignedPathId ? (
              <Badge variant="outline">
                {settings.paths.find((path) => path.id === device.assignedPathId)
                  ?.name ?? device.assignedPathId}
              </Badge>
            ) : null}
          </div>
        </div>
        <div className="flex flex-wrap justify-end gap-2">
          <Button variant="outline" size="sm" onClick={onSelect}>
            {t(WEBUI.standardDevices.viewQueries)}
          </Button>
          <Button variant="outline" size="icon" onClick={onRemove}>
            <Trash2 className="size-4" />
            <span className="sr-only">{t(WEBUI.standardDevices.removeDevice)}</span>
          </Button>
        </div>
      </CardHeader>
      <CardContent className="grid gap-4 lg:grid-cols-2">
        <div className="space-y-2">
          <Label htmlFor={`${device.id}-name`}>
            {t(WEBUI.standardDevices.deviceName)}
          </Label>
          <Input
            id={`${device.id}-name`}
            value={device.name}
            onChange={(event) => onUpdate({ name: event.target.value })}
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor={`${device.id}-path`}>
            {t(WEBUI.standardDevices.assignedPath)}
          </Label>
          <Select
            value={device.assignedPathId ?? UNASSIGNED_PATH}
            onValueChange={(value) =>
              onUpdate({
                assignedPathId: value === UNASSIGNED_PATH ? undefined : value,
              })
            }
          >
            <SelectTrigger id={`${device.id}-path`}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={UNASSIGNED_PATH}>
                {t(WEBUI.standardDevices.noAssignedPath)}
              </SelectItem>
              {settings.paths.map((path) => (
                <SelectItem key={path.id} value={path.id}>
                  {path.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-2 lg:col-span-2">
          <Label htmlFor={`${device.id}-addresses`}>
            {t(WEBUI.standardDevices.addresses)}
          </Label>
          <Textarea
            id={`${device.id}-addresses`}
            className="min-h-24 font-mono"
            value={device.addresses.join("\n")}
            onChange={(event) => onUpdate({ addresses: lines(event.target.value) })}
            placeholder={"192.168.1.20\n192.168.1.0/24"}
          />
        </div>
        <div className="space-y-2">
          <Label>{t(WEBUI.standardDevices.filteringPolicy)}</Label>
          <Select
            value={device.filtering ?? "inherit"}
            onValueChange={(value) =>
              onUpdate({
                filtering: value as NonNullable<StandardDeviceProfile["filtering"]>,
              })
            }
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(["inherit", "enabled", "disabled"] as const).map((policy) => (
                <SelectItem key={policy} value={policy}>
                  {t(policyLabelKeys[policy])}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-2">
          <Label>{t(WEBUI.standardDevices.queryLogPolicy)}</Label>
          <Select
            value={device.queryLog ?? "inherit"}
            onValueChange={(value) =>
              onUpdate({
                queryLog: value as NonNullable<StandardDeviceProfile["queryLog"]>,
              })
            }
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(["inherit", "enabled", "disabled"] as const).map((policy) => (
                <SelectItem key={policy} value={policy}>
                  {t(policyLabelKeys[policy])}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex flex-wrap gap-2 lg:col-span-2">
          <Button variant="outline" size="sm" onClick={onChildProtection}>
            <Baby className="size-4" />
            {t(WEBUI.standardDevices.childProtection)}
          </Button>
          <Button variant="outline" size="sm" onClick={onPrivacy}>
            <ShieldOff className="size-4" />
            {t(WEBUI.standardDevices.privacyPreset)}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

function MetricCard({ label, value }: { label: string; value: string }) {
  return (
    <Card>
      <CardContent className="p-4">
        <div className="text-xs text-muted-foreground">{label}</div>
        <div className="mt-1 text-2xl font-semibold">{value}</div>
      </CardContent>
    </Card>
  );
}

function ValidationPanel({
  issues,
  saveError,
}: {
  issues: StandardDeviceValidationIssue[];
  saveError: string | null;
}) {
  const { t } = useI18n();
  return (
    <div className="rounded-lg border border-destructive/30 bg-destructive/10 p-4 text-sm text-destructive">
      <div className="font-medium">{t(WEBUI.standardDevices.validationTitle)}</div>
      <ul className="mt-2 list-disc space-y-1 pl-5">
        {issues.map((issue, index) => (
          <li key={`${issue.code}-${issue.deviceId ?? issue.field}-${index}`}>
            {validationMessage(issue, t)}
          </li>
        ))}
        {saveError ? <li>{saveError}</li> : null}
      </ul>
    </div>
  );
}

function validationMessage(
  issue: StandardDeviceValidationIssue,
  t: ReturnType<typeof useI18n>["t"],
) {
  const keyByCode: Record<StandardDeviceValidationIssue["code"], string> = {
    capability_required: WEBUI.standardDevices.validationCapabilityRequired,
    device_name_required: WEBUI.standardDevices.validationNameRequired,
    device_address_required: WEBUI.standardDevices.validationAddressRequired,
    device_address_invalid: WEBUI.standardDevices.validationAddressInvalid,
    device_path_required: WEBUI.standardDevices.validationPathRequired,
    filtering_capability_required:
      WEBUI.standardDevices.validationFilteringCapabilityRequired,
    filtering_rule_source_required:
      WEBUI.standardDevices.validationFilteringRuleSourceRequired,
    filtering_subscription_runtime_required:
      WEBUI.standardDevices.validationFilteringSubscriptionRuntimeRequired,
    query_log_capability_required:
      WEBUI.standardDevices.validationQueryLogCapabilityRequired,
  };
  return t(keyByCode[issue.code]);
}
