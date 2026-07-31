"use client";

import { useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { Loader2, Plus, Save, Trash2, WandSparkles } from "lucide-react";
import { AppHeader } from "@/components/shell/app-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import {
  appendDynamicDomainRules,
  clearDynamicDomainRules,
  fetchDynamicDomainStatus,
  listDynamicDomainRules,
  previewStandardTemplate,
  removeDynamicDomainRules,
  setLearnDomainPaused,
  type DynamicDomainStatusResponse,
} from "@/lib/oxidns-api";
import type {
  StandardAdvancedCondition,
  StandardAdvancedRule,
  StandardDedicatedGroup,
  StandardDynamicLearningProfile,
  StandardModeSettings,
  StandardTemplateKind,
  StandardTemplatePreviewResponse,
} from "@/lib/standard-mode/types";
import { useAppStore } from "@/lib/store";

function rows(value: string) {
  return value.split("\n").map((row) => row.trim()).filter(Boolean);
}

function uniqueId(prefix: string, ids: string[]) {
  let index = ids.length + 1;
  while (ids.includes(`${prefix}_${index}`)) index += 1;
  return `${prefix}_${index}`;
}

function newLearning(settings: StandardModeSettings): StandardDynamicLearningProfile {
  const id = uniqueId("learning", settings.dynamicLearning.profiles.map((item) => item.id));
  return {
    id,
    name: id,
    enabled: true,
    paused: false,
    targetPathId: settings.paths[0]?.id ?? "default",
    priority: 100,
    qtypes: ["A", "AAAA"],
    rcodes: ["NOERROR"],
    answerRequired: true,
    ruleKind: "domain",
    maxEntries: 10_000,
    entryTtlSeconds: 86_400,
    cleanupIntervalSeconds: 600,
    queueSize: 1024,
    batchSize: 128,
    flushIntervalMs: 200,
    failurePolicy: "continue",
  };
}

function newDedicatedGroup(settings: StandardModeSettings): StandardDedicatedGroup {
  const id = uniqueId("dedicated", settings.dedicatedGroups.map((item) => item.id));
  const basePath = settings.paths[0];
  const baseGroup = settings.upstreamGroups.find((item) => item.id === basePath?.upstreamGroupId)
    ?? settings.upstreamGroups[0];
  return {
    id,
    name: id,
    enabled: true,
    priority: 100,
    rules: ["domain:example.com"],
    strategy: baseGroup?.strategy ?? "balanced",
    upstreams: structuredClone(baseGroup?.upstreams ?? []),
    path: {
      filtering: basePath?.filtering ?? "inherit",
      cache: basePath?.cache ?? "inherit",
      queryLog: basePath?.queryLog ?? "inherit",
      dualStack: basePath?.dualStack ?? "inherit",
      ipSelection: structuredClone(basePath?.ipSelection ?? {
        enabled: false,
        selectionMode: "first_success",
        probeMethods: ["tcp:443"],
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
      }),
      ecs: structuredClone(basePath?.ecs ?? { mode: "inherit" }),
    },
    listener: { enabled: false, address: "127.0.0.1:5539", udp: true, tcp: true },
  };
}

function newAdvancedRule(settings: StandardModeSettings, phase: "request" | "response"): StandardAdvancedRule {
  const id = uniqueId("advanced", settings.advancedRules.map((item) => item.id));
  const target = settings.paths[1]?.id ?? settings.paths[0]?.id ?? "default";
  return {
    id,
    name: id,
    enabled: true,
    priority: 100,
    phase,
    conditions: phase === "request"
      ? [{ type: "qtype", values: ["A", "AAAA"] }]
      : [
          { type: "source_path", pathId: settings.paths[0]?.id ?? "default" },
          { type: "rcode", values: ["SERVFAIL"] },
        ],
    action: { type: "use_path", pathId: target },
    failurePolicy: "fail_open",
    failureResponse: "servfail",
  };
}

export default function StandardAdvancedPage() {
  const stored = useAppStore((state) => state.standardSettings);
  const generated = useAppStore((state) => state.standardLastGenerated);
  const saveStandardSettings = useAppStore((state) => state.saveStandardSettings);
  const isConfigSaving = useAppStore((state) => state.isConfigSaving);
  const isApplying = useAppStore((state) => state.isApplying);
  const { t } = useI18n();
  const [draft, setDraft] = useState<StandardModeSettings | null>(null);
  const [kind, setKind] = useState<StandardTemplateKind>("low_latency");
  const [namespace, setNamespace] = useState("scenario_1");
  const [domains, setDomains] = useState("domain:example.com");
  const [encryptedAddress, setEncryptedAddress] = useState("");
  const [preview, setPreview] = useState<StandardTemplatePreviewResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const settings = draft ?? stored;
  const busy = isConfigSaving || isApplying;
  const baseUpstreams = useMemo(() => {
    const values = structuredClone(settings.upstreamGroups[0]?.upstreams ?? []);
    if (kind === "privacy_dns") {
      if (!encryptedAddress.trim()) return [];
      return values.slice(0, 1).map((upstream) => ({
        ...upstream,
        protocol: "doh" as const,
        address: encryptedAddress.trim(),
        dohPath: "/dns-query",
      }));
    }
    return values;
  }, [encryptedAddress, kind, settings.upstreamGroups]);

  const runPreview = async () => {
    setPreviewing(true);
    setError(null);
    try {
      setPreview(await previewStandardTemplate({
        baseIntent: settings,
        kind,
        parameters: {
          namespace,
          name: namespace,
          domains: rows(domains),
          upstreams: baseUpstreams,
          ...(kind === "internal_domains" ? { listenerAddress: "127.0.0.1:5539" } : {}),
        },
        takeover: true,
      }));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setPreviewing(false);
    }
  };

  const save = async () => {
    setError(null);
    try {
      await saveStandardSettings(settings, { apply: true });
      setDraft(settings);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  return <>
    <AppHeader title={t(WEBUI.standardAdvanced.title)} />
    <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
      <div className="mx-auto max-w-6xl space-y-6">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h1 className="text-xl font-semibold">{t(WEBUI.standardAdvanced.title)}</h1>
            <p className="mt-1 text-sm text-muted-foreground">{t(WEBUI.standardAdvanced.description)}</p>
          </div>
          <Button onClick={() => void save()} disabled={busy}>
            {busy ? <Loader2 className="size-4 animate-spin" /> : <Save className="size-4" />}
            {t(WEBUI.standardAdvanced.saveApply)}
          </Button>
        </div>
        <div className="rounded-lg border bg-muted/30 p-3 text-sm text-muted-foreground">{t(WEBUI.standardAdvanced.nativeBoundary)}</div>
        {error ? <div className="rounded-lg border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">{error}</div> : null}

        <Card>
          <CardHeader><CardTitle className="flex items-center gap-2 text-base"><WandSparkles className="size-4" />{t(WEBUI.standardAdvanced.templates)}</CardTitle></CardHeader>
          <CardContent className="space-y-4">
            <p className="text-sm text-muted-foreground">{t(WEBUI.standardAdvanced.templateHint)}</p>
            <div className="grid gap-3 md:grid-cols-3">
              <Select value={kind} onValueChange={(value) => setKind(value as StandardTemplateKind)}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="low_latency">Low latency</SelectItem>
                  <SelectItem value="privacy_dns">Privacy DNS</SelectItem>
                  <SelectItem value="internal_domains">Internal domains</SelectItem>
                  <SelectItem value="regional_upstream">Regional upstream</SelectItem>
                </SelectContent>
              </Select>
              <Input value={namespace} onChange={(event) => setNamespace(event.target.value)} placeholder={t(WEBUI.standardAdvanced.namespace)} />
              <Button variant="outline" onClick={() => void runPreview()} disabled={previewing}>
                {previewing ? <Loader2 className="size-4 animate-spin" /> : <WandSparkles className="size-4" />}
                {t(WEBUI.standardAdvanced.preview)}
              </Button>
            </div>
            <div><Label>{t(WEBUI.standardAdvanced.domains)}</Label><Textarea className="mt-2" value={domains} onChange={(event) => setDomains(event.target.value)} /></div>
            {kind === "privacy_dns" ? <div><Label>Encrypted upstream URL</Label><Input className="mt-2" value={encryptedAddress} onChange={(event) => setEncryptedAddress(event.target.value)} placeholder="https://resolver.example/dns-query" /></div> : null}
            {preview ? <div className="rounded-lg border p-3 text-sm">
              <div className="flex flex-wrap items-center gap-2">
                <span>{t(WEBUI.standardAdvanced.previewObjects)}:</span>
                {preview.expansion.objectsAdded.map((item) => <Badge key={item} variant="secondary">{item}</Badge>)}
                <Button className="ml-auto" size="sm" onClick={() => { setDraft(preview.expansion.proposedIntent); setPreview(null); }}>
                  {t(WEBUI.standardAdvanced.acceptDraft)}
                </Button>
              </div>
              {preview.plan.plan.diagnostics.map((item) => <p key={`${item.code}:${item.path}`} className="mt-2 text-muted-foreground">{item.code}: {item.message}</p>)}
            </div> : null}
          </CardContent>
        </Card>

        <PolicyList title={t(WEBUI.standardAdvanced.dedicated)} empty={t(WEBUI.standardAdvanced.empty)} action={<Button size="sm" variant="outline" onClick={() => setDraft({ ...settings, dedicatedGroups: [...settings.dedicatedGroups, newDedicatedGroup(settings)] })}><Plus className="size-4" />Add dedicated group</Button>}>
          {settings.dedicatedGroups.map((group) => <DedicatedRow key={group.id} group={group} onChange={(next) => setDraft({ ...settings, dedicatedGroups: settings.dedicatedGroups.map((item) => item.id === group.id ? next : item) })} onRemove={() => setDraft({ ...settings, dedicatedGroups: settings.dedicatedGroups.filter((item) => item.id !== group.id) })} removeLabel={t(WEBUI.standardAdvanced.remove)} />)}
        </PolicyList>

        <PolicyList title={t(WEBUI.standardAdvanced.learning)} empty={t(WEBUI.standardAdvanced.empty)} action={<Button size="sm" variant="outline" onClick={() => setDraft({ ...settings, dynamicLearning: { profiles: [...settings.dynamicLearning.profiles, newLearning(settings)] } })}><Plus className="size-4" />{t(WEBUI.standardAdvanced.addLearning)}</Button>}>
          {settings.dynamicLearning.profiles.map((profile) => <LearningRow key={profile.id} profile={profile} settings={settings} t={t} onChange={(next) => setDraft({ ...settings, dynamicLearning: { profiles: settings.dynamicLearning.profiles.map((item) => item.id === profile.id ? next : item) } })} onRemove={() => setDraft({ ...settings, dynamicLearning: { profiles: settings.dynamicLearning.profiles.filter((item) => item.id !== profile.id) } })} />)}
        </PolicyList>

        {settings.dynamicLearning.profiles.map((profile) => {
          const tags = generated?.tagMap.dynamicLearning?.[profile.id];
          return tags ? <LearningRuntime key={`runtime:${profile.id}`} profile={profile} providerTag={tags.provider} learnerTag={tags.learner} /> : null;
        })}

        <PolicyList title={t(WEBUI.standardAdvanced.advancedRules)} empty={t(WEBUI.standardAdvanced.empty)} action={<div className="flex gap-2"><Button size="sm" variant="outline" onClick={() => setDraft({ ...settings, advancedRules: [...settings.advancedRules, newAdvancedRule(settings, "request")] })}>{t(WEBUI.standardAdvanced.addRequestRule)}</Button><Button size="sm" variant="outline" onClick={() => setDraft({ ...settings, advancedRules: [...settings.advancedRules, newAdvancedRule(settings, "response")] })}>{t(WEBUI.standardAdvanced.addResponseRule)}</Button></div>}>
          {settings.advancedRules.map((rule) => <AdvancedRuleRow key={rule.id} rule={rule} settings={settings} onChange={(next) => setDraft({ ...settings, advancedRules: settings.advancedRules.map((item) => item.id === rule.id ? next : item) })} onRemove={() => setDraft({ ...settings, advancedRules: settings.advancedRules.filter((item) => item.id !== rule.id) })} removeLabel={t(WEBUI.standardAdvanced.remove)} />)}
        </PolicyList>
      </div>
    </main>
  </>;
}

function LearningRuntime({ profile, providerTag, learnerTag }: { profile: StandardDynamicLearningProfile; providerTag: string; learnerTag: string }) {
  const [status, setStatus] = useState<DynamicDomainStatusResponse | null>(null);
  const [rules, setRules] = useState<string[]>([]);
  const [nextCursor, setNextCursor] = useState<number | null>(null);
  const [correction, setCorrection] = useState("");
  const [paused, setPaused] = useState(profile.paused);
  const [error, setError] = useState<string | null>(null);
  const load = async (cursor = 0, append = false) => {
    try {
      const [nextStatus, page] = await Promise.all([
        fetchDynamicDomainStatus(providerTag),
        listDynamicDomainRules(providerTag, { cursor, limit: 20 }),
      ]);
      setStatus(nextStatus);
      setRules((current) => append ? [...current, ...page.rules] : page.rules);
      setNextCursor(page.next_cursor);
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };
  useEffect(() => {
    let active = true;
    void Promise.all([
      fetchDynamicDomainStatus(providerTag),
      listDynamicDomainRules(providerTag, { cursor: 0, limit: 20 }),
    ]).then(([nextStatus, page]) => {
      if (!active) return;
      setStatus(nextStatus);
      setRules(page.rules);
      setNextCursor(page.next_cursor);
      setError(null);
    }).catch((cause: unknown) => {
      if (active) setError(cause instanceof Error ? cause.message : String(cause));
    });
    return () => { active = false; };
  }, [providerTag]);
  const mutate = async (action: () => Promise<unknown>) => {
    try { await action(); await load(); setCorrection(""); } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
  };
  return <Card>
    <CardHeader className="flex flex-row items-center justify-between"><CardTitle className="text-base">{profile.name} · runtime</CardTitle><Button size="sm" variant="outline" onClick={() => void load()}>Refresh</Button></CardHeader>
    <CardContent className="space-y-3">
      {error ? <p className="text-sm text-destructive">{error}</p> : null}
      <div className="flex flex-wrap gap-2 text-sm">
        <Badge variant="secondary">total {status?.total ?? "-"}</Badge>
        <Badge variant="secondary">learned {status?.learned ?? "-"}</Badge>
        <Badge variant="secondary">manual {status?.manual ?? "-"}</Badge>
        <Badge variant="secondary">expired {status?.expiredTotal ?? "-"}</Badge>
        <Badge variant="secondary">rejected {(status?.capacityRejectedTotal ?? 0) + (status?.queueRejectedTotal ?? 0)}</Badge>
      </div>
      <div className="flex flex-wrap gap-2"><Input className="min-w-52 flex-1" value={correction} onChange={(event) => setCorrection(event.target.value)} placeholder="example.com" /><Button size="sm" variant="outline" disabled={!correction.trim()} onClick={() => void mutate(() => appendDynamicDomainRules(providerTag, [correction.trim()], profile.ruleKind))}>Add manual correction</Button><Button size="sm" variant="outline" disabled={!correction.trim()} onClick={() => void mutate(() => removeDynamicDomainRules(providerTag, [correction.trim()], profile.ruleKind))}>Remove</Button><Button size="sm" variant="outline" onClick={() => void mutate(async () => { const result = await setLearnDomainPaused(learnerTag, !paused); setPaused(result.paused); })}>{paused ? "Resume" : "Pause"}</Button><Button size="sm" variant="destructive" onClick={() => void mutate(() => clearDynamicDomainRules(providerTag))}>Clear</Button></div>
      <div className="flex flex-wrap gap-2">{rules.map((rule) => <Badge key={rule} variant="outline">{rule}</Badge>)}</div>
      {nextCursor != null ? <Button size="sm" variant="ghost" onClick={() => void load(nextCursor, true)}>Load more</Button> : null}
      {status?.lastError ? <p className="text-sm text-destructive">{status.lastError}</p> : null}
    </CardContent>
  </Card>;
}

function PolicyList({ title, empty, action, children }: { title: string; empty: string; action?: ReactNode; children: ReactNode }) {
  const hasChildren = Array.isArray(children) ? children.length > 0 : Boolean(children);
  return <Card><CardHeader className="flex flex-row items-center justify-between"><CardTitle className="text-base">{title}</CardTitle>{action}</CardHeader><CardContent className="space-y-3">{hasChildren ? children : <p className="text-sm text-muted-foreground">{empty}</p>}</CardContent></Card>;
}

function LearningRow({ profile, settings, onChange, onRemove, t }: { profile: StandardDynamicLearningProfile; settings: StandardModeSettings; onChange: (value: StandardDynamicLearningProfile) => void; onRemove: () => void; t: (key: string) => string }) {
  return <div className="space-y-3 rounded-lg border p-3">
    <div className="flex items-center gap-3"><Input value={profile.name} onChange={(event) => onChange({ ...profile, name: event.target.value })} /><Button size="sm" variant="ghost" onClick={onRemove}><Trash2 className="size-4" />{t(WEBUI.standardAdvanced.remove)}</Button></div>
    <div className="grid gap-3 md:grid-cols-4">
      <div><Label>{t(WEBUI.standardAdvanced.targetPath)}</Label><Select value={profile.targetPathId} onValueChange={(targetPathId) => onChange({ ...profile, targetPathId })}><SelectTrigger className="mt-2"><SelectValue /></SelectTrigger><SelectContent>{settings.paths.map((path) => <SelectItem key={path.id} value={path.id}>{path.name}</SelectItem>)}</SelectContent></Select></div>
      <div><Label>{t(WEBUI.standardAdvanced.maxEntries)}</Label><Input className="mt-2" type="number" value={profile.maxEntries} onChange={(event) => onChange({ ...profile, maxEntries: Number(event.target.value) })} /></div>
      <div><Label>{t(WEBUI.standardAdvanced.entryTtl)}</Label><Input className="mt-2" type="number" value={profile.entryTtlSeconds} onChange={(event) => onChange({ ...profile, entryTtlSeconds: Number(event.target.value) })} /></div>
      <Label className="mt-6 flex items-center justify-between rounded-lg border px-3">{t(WEBUI.standardAdvanced.paused)}<Switch checked={profile.paused} onCheckedChange={(paused) => onChange({ ...profile, paused })} /></Label>
    </div>
    <div className="grid gap-3 md:grid-cols-4">
      <div><Label>Accepted QTYPEs</Label><Input className="mt-2" value={profile.qtypes.join(", ")} onChange={(event) => onChange({ ...profile, qtypes: event.target.value.split(",").map((item) => item.trim()).filter(Boolean) })} /></div>
      <div><Label>Accepted RCODEs</Label><Input className="mt-2" value={profile.rcodes.join(", ")} onChange={(event) => onChange({ ...profile, rcodes: event.target.value.split(",").map((item) => item.trim()).filter(Boolean) })} /></div>
      <div><Label>Failure policy</Label><Select value={profile.failurePolicy} onValueChange={(failurePolicy) => onChange({ ...profile, failurePolicy: failurePolicy as StandardDynamicLearningProfile["failurePolicy"] })}><SelectTrigger className="mt-2"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="continue">Continue DNS</SelectItem><SelectItem value="fail_closed">Fail closed</SelectItem></SelectContent></Select></div>
      <div><Label>Rule kind</Label><Select value={profile.ruleKind} onValueChange={(ruleKind) => onChange({ ...profile, ruleKind: ruleKind as StandardDynamicLearningProfile["ruleKind"] })}><SelectTrigger className="mt-2"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="full">Exact domain</SelectItem><SelectItem value="domain">Domain suffix</SelectItem></SelectContent></Select></div>
    </div>
    <div className="grid gap-3 md:grid-cols-4">
      <div><Label>Cleanup interval (s)</Label><Input className="mt-2" type="number" value={profile.cleanupIntervalSeconds} onChange={(event) => onChange({ ...profile, cleanupIntervalSeconds: Number(event.target.value) })} /></div>
      <div><Label>Queue size</Label><Input className="mt-2" type="number" value={profile.queueSize} onChange={(event) => onChange({ ...profile, queueSize: Number(event.target.value) })} /></div>
      <div><Label>Batch size</Label><Input className="mt-2" type="number" value={profile.batchSize} onChange={(event) => onChange({ ...profile, batchSize: Number(event.target.value) })} /></div>
      <div><Label>Flush interval (ms)</Label><Input className="mt-2" type="number" value={profile.flushIntervalMs} onChange={(event) => onChange({ ...profile, flushIntervalMs: Number(event.target.value) })} /></div>
    </div>
    <div className="flex flex-wrap gap-4"><Label className="flex items-center gap-2"><Switch checked={profile.enabled} onCheckedChange={(enabled) => onChange({ ...profile, enabled })} />Enabled</Label><Label className="flex items-center gap-2"><Switch checked={profile.answerRequired} onCheckedChange={(answerRequired) => onChange({ ...profile, answerRequired })} />Require wanted answer</Label></div>
  </div>;
}

function DedicatedRow({ group, onChange, onRemove, removeLabel }: { group: StandardDedicatedGroup; onChange: (value: StandardDedicatedGroup) => void; onRemove: () => void; removeLabel: string }) {
  return <div className="space-y-3 rounded-lg border p-3">
    <div className="flex items-center gap-3"><Input value={group.name} onChange={(event) => onChange({ ...group, name: event.target.value })} /><Label className="flex items-center gap-2"><Switch checked={group.enabled} onCheckedChange={(enabled) => onChange({ ...group, enabled })} />Enabled</Label><Button size="sm" variant="ghost" onClick={onRemove}><Trash2 className="size-4" />{removeLabel}</Button></div>
    <div className="grid gap-3 md:grid-cols-2"><div><Label>Domain rules</Label><Textarea className="mt-2" value={group.rules.join("\n")} onChange={(event) => onChange({ ...group, rules: rows(event.target.value) })} /></div><div className="space-y-2"><Label>Dedicated upstream addresses</Label>{group.upstreams.map((upstream) => <Input key={upstream.id} value={upstream.address} onChange={(event) => onChange({ ...group, upstreams: group.upstreams.map((item) => item.id === upstream.id ? { ...item, address: event.target.value } : item) })} />)}</div></div>
    <div className="grid gap-3 md:grid-cols-4">
      <div><Label>Strategy</Label><Select value={group.strategy} onValueChange={(strategy) => onChange({ ...group, strategy: strategy as StandardDedicatedGroup["strategy"] })}><SelectTrigger className="mt-2"><SelectValue /></SelectTrigger><SelectContent>{["fastest", "balanced", "prefer_positive", "consensus", "ordered_fallback"].map((value) => <SelectItem key={value} value={value}>{value}</SelectItem>)}</SelectContent></Select></div>
      <div><Label>Cache</Label><Select value={group.path.cache} onValueChange={(cache) => onChange({ ...group, path: { ...group.path, cache: cache as StandardDedicatedGroup["path"]["cache"] } })}><SelectTrigger className="mt-2"><SelectValue /></SelectTrigger><SelectContent>{["inherit", "enabled", "disabled"].map((value) => <SelectItem key={value} value={value}>{value}</SelectItem>)}</SelectContent></Select></div>
      <div><Label>Filtering</Label><Select value={group.path.filtering} onValueChange={(filtering) => onChange({ ...group, path: { ...group.path, filtering: filtering as StandardDedicatedGroup["path"]["filtering"] } })}><SelectTrigger className="mt-2"><SelectValue /></SelectTrigger><SelectContent>{["inherit", "enabled", "disabled"].map((value) => <SelectItem key={value} value={value}>{value}</SelectItem>)}</SelectContent></Select></div>
      <div><Label>Priority</Label><Input className="mt-2" type="number" value={group.priority} onChange={(event) => onChange({ ...group, priority: Number(event.target.value) })} /></div>
    </div>
    <div className="grid gap-3 md:grid-cols-4"><Label className="mt-6 flex items-center gap-2"><Switch checked={group.listener.enabled} onCheckedChange={(enabled) => onChange({ ...group, listener: { ...group.listener, enabled } })} />Native listener</Label><div><Label>Loopback/address</Label><Input className="mt-2" value={group.listener.address} onChange={(event) => onChange({ ...group, listener: { ...group.listener, address: event.target.value } })} /></div><Label className="mt-6 flex items-center gap-2"><Switch checked={group.listener.udp} onCheckedChange={(udp) => onChange({ ...group, listener: { ...group.listener, udp } })} />UDP</Label><Label className="mt-6 flex items-center gap-2"><Switch checked={group.listener.tcp} onCheckedChange={(tcp) => onChange({ ...group, listener: { ...group.listener, tcp } })} />TCP</Label></div>
  </div>;
}

const requestConditionTypes = ["domain", "suffix", "keyword", "client_cidr", "qtype", "time", "rate_limit_exceeded"] as const;
const responseConditionTypes = ["source_path", "cname", "rcode", "has_wanted_answer", "response_ip_role"] as const;

function conditionFor(type: StandardAdvancedCondition["type"], settings: StandardModeSettings): StandardAdvancedCondition {
  switch (type) {
    case "source_path": return { type, pathId: settings.paths[0]?.id ?? "default" };
    case "has_wanted_answer": return { type };
    case "response_ip_role": return { type, role: "domestic_ips", invert: false };
    case "time": return { type, timezone: "UTC", periods: [{ start: "00:00", end: "23:59", weekdays: [], monthdays: [] }] };
    case "rate_limit_exceeded": return { type, qps: 100, burst: 200, mask4: 32, mask6: 64 };
    default: return { type, values: type === "qtype" ? ["A", "AAAA"] : type === "rcode" ? ["SERVFAIL"] : ["example.com"] };
  }
}

function AdvancedRuleRow({ rule, settings, onChange, onRemove, removeLabel }: { rule: StandardAdvancedRule; settings: StandardModeSettings; onChange: (value: StandardAdvancedRule) => void; onRemove: () => void; removeLabel: string }) {
  const allowedTypes = rule.phase === "request" ? requestConditionTypes : responseConditionTypes;
  const updateCondition = (index: number, condition: StandardAdvancedCondition) => onChange({ ...rule, conditions: rule.conditions.map((item, itemIndex) => itemIndex === index ? condition : item) });
  return <div className="space-y-3 rounded-lg border p-3">
    <div className="flex flex-wrap items-center gap-3"><Input className="min-w-52 flex-1" value={rule.name} onChange={(event) => onChange({ ...rule, name: event.target.value })} /><Badge>{rule.phase}</Badge><Label className="flex items-center gap-2"><Switch checked={rule.enabled} onCheckedChange={(enabled) => onChange({ ...rule, enabled })} />Enabled</Label><Button size="sm" variant="ghost" onClick={onRemove}><Trash2 className="size-4" />{removeLabel}</Button></div>
    <div className="space-y-2">{rule.conditions.map((condition, index) => <div key={`${condition.type}:${index}`} className="grid gap-2 rounded border p-2 md:grid-cols-[12rem_1fr_auto]">
      <Select value={condition.type} onValueChange={(type) => updateCondition(index, conditionFor(type as StandardAdvancedCondition["type"], settings))}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent>{allowedTypes.map((type) => <SelectItem key={type} value={type}>{type}</SelectItem>)}</SelectContent></Select>
      <ConditionFields condition={condition} settings={settings} onChange={(next) => updateCondition(index, next)} />
      <Button size="icon" variant="ghost" aria-label="Remove condition" onClick={() => onChange({ ...rule, conditions: rule.conditions.filter((_, itemIndex) => itemIndex !== index) })}><Trash2 className="size-4" /></Button>
    </div>)}</div>
    <Button size="sm" variant="outline" onClick={() => onChange({ ...rule, conditions: [...rule.conditions, conditionFor(allowedTypes[0], settings)] })}><Plus className="size-4" />Add AND condition</Button>
    <div className="grid gap-3 md:grid-cols-4"><div><Label>Action</Label><Select value={rule.action.type} onValueChange={(type) => onChange({ ...rule, action: type === "block" ? { type: "block", response: "refused" } : { type: "use_path", pathId: settings.paths[0]?.id ?? "default" } })}><SelectTrigger className="mt-2"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="use_path">Use path</SelectItem>{rule.phase === "request" ? <SelectItem value="block">Block</SelectItem> : null}</SelectContent></Select></div>{rule.action.type === "use_path" ? <div><Label>Target path</Label><Select value={rule.action.pathId} onValueChange={(pathId) => onChange({ ...rule, action: { type: "use_path", pathId } })}><SelectTrigger className="mt-2"><SelectValue /></SelectTrigger><SelectContent>{settings.paths.map((path) => <SelectItem key={path.id} value={path.id}>{path.name}</SelectItem>)}</SelectContent></Select></div> : <div><Label>Block response</Label><Select value={rule.action.response} onValueChange={(response) => onChange({ ...rule, action: { type: "block", response: response as Extract<StandardAdvancedRule["action"], { type: "block" }>["response"] } })}><SelectTrigger className="mt-2"><SelectValue /></SelectTrigger><SelectContent>{["null_ip", "nxdomain", "nodata", "refused"].map((value) => <SelectItem key={value} value={value}>{value}</SelectItem>)}</SelectContent></Select></div>}<div><Label>Failure policy</Label><Select value={rule.failurePolicy} onValueChange={(failurePolicy) => onChange({ ...rule, failurePolicy: failurePolicy as StandardAdvancedRule["failurePolicy"] })}><SelectTrigger className="mt-2"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="fail_open">Preserve original</SelectItem><SelectItem value="fail_closed">Fail closed</SelectItem></SelectContent></Select></div><div><Label>Priority</Label><Input className="mt-2" type="number" value={rule.priority} onChange={(event) => onChange({ ...rule, priority: Number(event.target.value) })} /></div></div>
  </div>;
}

function ConditionFields({ condition, settings, onChange }: { condition: StandardAdvancedCondition; settings: StandardModeSettings; onChange: (value: StandardAdvancedCondition) => void }) {
  if ("values" in condition) return <Input value={condition.values.join(", ")} onChange={(event) => onChange({ ...condition, values: event.target.value.split(",").map((item) => item.trim()).filter(Boolean) })} />;
  if (condition.type === "source_path") return <Select value={condition.pathId} onValueChange={(pathId) => onChange({ ...condition, pathId })}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent>{settings.paths.map((path) => <SelectItem key={path.id} value={path.id}>{path.name}</SelectItem>)}</SelectContent></Select>;
  if (condition.type === "response_ip_role") return <div className="flex gap-2"><Input value={condition.role} onChange={(event) => onChange({ ...condition, role: event.target.value })} /><Label className="flex items-center gap-2"><Switch checked={condition.invert} onCheckedChange={(invert) => onChange({ ...condition, invert })} />Invert</Label></div>;
  if (condition.type === "rate_limit_exceeded") return <div className="grid grid-cols-4 gap-2"><Input aria-label="QPS" type="number" value={condition.qps} onChange={(event) => onChange({ ...condition, qps: Number(event.target.value) })} /><Input aria-label="Burst" type="number" value={condition.burst} onChange={(event) => onChange({ ...condition, burst: Number(event.target.value) })} /><Input aria-label="IPv4 mask" type="number" value={condition.mask4} onChange={(event) => onChange({ ...condition, mask4: Number(event.target.value) })} /><Input aria-label="IPv6 mask" type="number" value={condition.mask6} onChange={(event) => onChange({ ...condition, mask6: Number(event.target.value) })} /></div>;
  if (condition.type === "time") { const period = condition.periods[0] ?? { start: "00:00", end: "23:59", weekdays: [], monthdays: [] }; return <div className="grid grid-cols-3 gap-2"><Input aria-label="Timezone" value={condition.timezone} onChange={(event) => onChange({ ...condition, timezone: event.target.value })} /><Input aria-label="Start" type="time" value={period.start ?? ""} onChange={(event) => onChange({ ...condition, periods: [{ ...period, start: event.target.value }] })} /><Input aria-label="End" type="time" value={period.end ?? ""} onChange={(event) => onChange({ ...condition, periods: [{ ...period, end: event.target.value }] })} /></div>; }
  return <p className="self-center text-sm text-muted-foreground">No parameters</p>;
}
